//! 요금제 사용 한도 — `UsageStore` (`ClaudeBoard.swift UsageStore`, SPEC §5.4).
//! 30초마다 본다. 상태줄 스크립트와 같은 캐시 파일을 함께 써서 API 호출을 줄인다. 토큰은 메모리에만 두고 기록하지 않는다.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_core::clock::iso_date;
use claude_board_core::{clock, fileio, Record};
use serde_json::Value;

use crate::model::{Limit, UsageView};
use crate::util::{http_get, ms, mtime, num, obj};

#[cfg(test)]
mod table;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

pub struct Usage {
    pub limits: Vec<Limit>,
    pub failed: bool,
    /// 로그인 토큰이 만료된 채로 있다 — 세션이 다음 요청을 보내며 갱신해야 사용량을 받을 수 있다
    pub token_expired: bool,
    pub fetching: bool,
    pub generation: u64,
    /// 계정을 바꾼 직후 이 시각 전에 쓰인 캐시는 믿지 않는다 — 상태줄이 앞 계정 토큰으로 받던 값을 늦게 써 넣을 수 있다
    pub trust_after: f64,
    pub cache: PathBuf,
}

/// 새로 받아야 하면 받을 세대 번호를 돌려준다.
pub enum Next {
    Idle,
    Fetch(u64),
}

impl Usage {
    pub fn new(cache: PathBuf) -> Self {
        Usage { limits: Vec::new(), failed: false, token_expired: false, fetching: false, generation: 0, trust_after: f64::NEG_INFINITY, cache }
    }

    pub fn view(&self) -> UsageView {
        UsageView { limits: self.limits.clone(), failed: self.failed, token_expired: self.token_expired }
    }

    /// 캐시를 먼저 보고, 120초 넘게 낡았으면 새로 받으라고 알린다.
    pub fn refresh(&mut self, now: f64) -> Next {
        let written = mtime(&self.cache).filter(|m| *m > self.trust_after);
        let body = if written.is_some() { fs::read(&self.cache).ok() } else { None };
        let (limits, fetch) = check_cache(written, body.as_deref(), self.fetching, now);
        if let Some(limits) = limits {
            self.limits = limits;
        }
        if fetch {
            self.fetching = true;
            Next::Fetch(self.generation)
        } else {
            Next::Idle
        }
    }

    /// 받은 결과를 넣는다. 받는 사이 계정을 바꿨으면 false(다시 받아야 한다).
    pub fn finish(&mut self, generation: u64, data: Option<Vec<u8>>, expired: bool) -> bool {
        self.fetching = false;
        if generation != self.generation {
            return false;
        }
        self.token_expired = expired;
        match data.as_deref().and_then(parse) {
            Some(limits) => {
                self.limits = limits;
                let _ = fileio::write_atomic(&self.cache, ".claude-usage-cache", data.as_deref().unwrap_or_default());
                self.trust_after = f64::NEG_INFINITY; // 새 계정 토큰으로 직접 받은 값이다
                self.failed = false;
            }
            None if self.limits.is_empty() => self.failed = true,
            None => {}
        }
        true
    }

    /// 계정을 바꾼 뒤: 앞 계정 값이 든 캐시를 버리고 바로 새로 받는다.
    pub fn reset(&mut self, now: f64) {
        self.generation += 1;
        self.trust_after = now + 5.0;
        let _ = fs::remove_file(&self.cache);
        self.failed = false;
        self.limits.clear();
    }
}

/// 지금 로그인 토큰으로 사용량 API 를 부른다 — (응답 본문, 토큰 만료).
pub fn fetch(credentials: &Path) -> (Option<Vec<u8>>, bool) {
    let Some(rec) = fileio::read_record(credentials) else { return (None, false) };
    let Some(oauth) = obj(rec.get("claudeAiOauth")) else { return (None, false) };
    let Some(token) = oauth.get("accessToken").and_then(Value::as_str) else { return (None, false) };
    if num(oauth.get("expiresAt")).is_some_and(|exp| exp / 1000.0 < clock::now()) {
        return (None, true);
    }
    let auth = format!("Bearer {token}");
    let headers = [("Authorization", auth.as_str()), ("anthropic-beta", "oauth-2025-04-20")];
    match http_get(USAGE_URL, &headers, Duration::from_secs(10)) {
        Ok((200, body)) => (Some(body), false),
        _ => (None, false),
    }
}

/// 캐시로 판단하는 순수 함수, (믿어 쓸 한도, 새로 받을지).
/// `written` 은 trust_after 뒤에 쓰인 캐시의 수정 시각(None 이면 못 믿는다), `body` 는 그때 읽은 본문.
/// 믿을 만하게 쓰인 캐시일 때만 본문을 한도로 해석한다. 나이는 푼 캐시만 센다: 못 푼(깨진·빈) 캐시는
/// 수정 시각이 낭신해도 믿을 수 없어 바로 받는다. 상태줄 스크립트가 같은 파일에 못 읽는 내용을
/// 계속 써도 수정 시각에 막혀 120초씩 기다리지 않는다. 미래의 수정 시각(시계 어긋남)은 나이가
/// 음수여도 적용은 받을지와 따로므로 그래도 받는다. 받는 중이면 무슨 경우든 받지 않는다.
pub fn check_cache(written: Option<f64>, body: Option<&[u8]>, fetching: bool, now: f64) -> (Option<Vec<Limit>>, bool) {
    // Some(빈 목록) 도 못 푼 캐시와 같이 취급해 나이를 세지 않는다
    let limits = if written.is_some() { body.and_then(parse) } else { None };
    let age = written.filter(|_| limits.is_some()).map_or(f64::INFINITY, |w| now - w);
    (limits, (age < 0.0 || age > 120.0) && !fetching)
}

fn limit(id: String, label: String, o: &Record, key: &str) -> Option<Limit> {
    let pct = num(o.get(key))?;
    let short = match id.as_str() {
        "5h" => "5시간".to_string(),
        "7d" => "주간".to_string(),
        _ => label.rsplit('·').next().unwrap_or(&label).trim().to_string(),
    };
    let tiny = match id.as_str() {
        "5h" => "5h".to_string(),
        "7d" => "7d".to_string(),
        _ => short.chars().take(1).collect(),
    };
    let resets = o.get("resets_at").and_then(Value::as_str).and_then(iso_date).map(ms);
    Some(Limit { id, label, short, tiny, pct, resets })
}

/// 아직 없는 id 를 하나 골라 쓰인 것으로 남긴다. `base` 가 이미 있으면 `base-2`, `base-3` … 로 물러난다.
fn take_unused_id(base: &str, used: &mut HashSet<String>) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while used.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    used.insert(id.clone());
    id
}

/// 사용량 API 응답을 한도 목록으로 — 5시간·주간 전체·주간 모델별. `five_hour` 가 없으면 None.
/// 항목마다 값이 숫자가 아니면 그 항목만 뺀다. 푼 한도가 하나도 없으면 None(실패): 빈 목록을
/// "성공" 으로 적용하고 캐시에 쓰면 보이던 한도가 사라지고 상태줄도 그 빈 캐시를 믿는다.
/// 5시간을 못 풀어도 나머지 한도가 풀리면 성공이다. id 는 화면 목록의 재배열 키라 유일해야 한다:
/// 모델별 주간의 이름이 같거나 이름이 없어 둘 다 "모델" 이면 7d-<이름> 이 겹치므로, 뺀 항목은
/// id 를 차지하지 않게 하고 남은 것을 응답 차례대로 처음 것은 그대로 두고 뒤의 것엔
/// -2·-3 … 을 붙인다. 라벨은 id 겹침과 무관하게 그대로 둔다.
pub fn parse(data: &[u8]) -> Option<Vec<Limit>> {
    let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(data) else { return None };
    let five = obj(j.get("five_hour"))?;
    let mut out = Vec::new();
    out.extend(limit("5h".into(), "5시간".into(), five, "utilization"));
    if let Some(seven) = obj(j.get("seven_day")) {
        out.extend(limit("7d".into(), "주간 · 전체".into(), seven, "utilization"));
    }
    for l in j.get("limits").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object) {
        if l.get("kind").and_then(Value::as_str) != Some("weekly_scoped") {
            continue;
        }
        let name = obj(l.get("scope"))
            .and_then(|s| obj(s.get("model")))
            .and_then(|m| m.get("display_name"))
            .and_then(Value::as_str)
            .unwrap_or("모델");
        out.extend(limit(format!("7d-{name}"), format!("주간 · {name}"), l, "percent"));
    }
    // 뺀 항목은 id 를 차지하지 않으므로 남은 것의 겹침만 여기서 푼다
    let mut used = HashSet::new();
    for l in out.iter_mut() {
        let id = take_unused_id(&l.id, &mut used);
        l.id = id;
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "five_hour": {"utilization": 38.0, "resets_at": "2026-10-16T14:30:00.512+00:00"},
      "seven_day": {"utilization": 54, "resets_at": "2026-10-19T00:00:00Z"},
      "seven_day_opus": null,
      "limits": [
        {"kind": "weekly_scoped", "percent": 12.5, "resets_at": "2026-10-19T00:00:00+09:00", "scope": {"model": {"display_name": "Fable"}}},
        {"kind": "monthly", "percent": 99},
        {"kind": "weekly_scoped", "percent": 3}
      ]
    }"#;

    #[test]
    fn parses_usage_response() {
        let l = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(l.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["5h", "7d", "7d-Fable", "7d-모델"]);
        assert_eq!((l[0].label.as_str(), l[0].short.as_str(), l[0].tiny.as_str(), l[0].pct), ("5시간", "5시간", "5h", 38.0));
        assert_eq!(l[0].resets, Some(1_792_161_000_512));
        assert_eq!((l[1].label.as_str(), l[1].short.as_str(), l[1].tiny.as_str()), ("주간 · 전체", "주간", "7d"));
        assert_eq!((l[2].label.as_str(), l[2].short.as_str(), l[2].tiny.as_str(), l[2].pct), ("주간 · Fable", "Fable", "F", 12.5));
        assert_eq!(l[2].resets, Some(1_792_335_600_000));
        assert_eq!(l[3].resets, None);
        assert!(parse(br#"{"seven_day": {"utilization": 1}}"#).is_none());
        assert!(parse(b"not json").is_none());
    }

    /// 견주기 쉽게 한도 id 만 뽑는다.
    fn ids(l: &[Limit]) -> Vec<&str> {
        l.iter().map(|l| l.id.as_str()).collect()
    }

    #[test]
    fn same_named_weekly_limits_get_unique_ids() {
        // 같은 이름이 셋이면 뒤의 것에 -2·-3 을 붙인다, 다른 이름 사이에 끼어도 차례대로
        let body = br#"{"five_hour":{"utilization":1},"limits":[
            {"kind":"weekly_scoped","percent":10,"scope":{"model":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":20,"scope":{"model":{"display_name":"Fable"}}},
            {"kind":"weekly_scoped","percent":30,"scope":{"model":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":40,"scope":{"model":{"display_name":"Opus"}}}]}"#;
        let l = parse(body).unwrap();
        assert_eq!(ids(&l), ["5h", "7d-Opus", "7d-Fable", "7d-Opus-2", "7d-Opus-3"]);
        // 라벨은 id 겹침과 무관하게 그대로 둔다
        assert_eq!(l[3].label, "주간 · Opus");
        assert_eq!(l[4].label, "주간 · Opus");
    }

    #[test]
    fn nameless_weekly_limits_get_unique_ids() {
        // 이름이 없어 둘 다 "모델" 이면 뒤의 것은 -2, 라벨은 그대로
        let body = br#"{"five_hour":{"utilization":1},"limits":[
            {"kind":"weekly_scoped","percent":1},
            {"kind":"weekly_scoped","percent":2}]}"#;
        let l = parse(body).unwrap();
        assert_eq!(ids(&l), ["5h", "7d-모델", "7d-모델-2"]);
        assert_eq!(l[1].label, "주간 · 모델");
        assert_eq!(l[2].label, "주간 · 모델");
    }

    #[test]
    fn suffix_avoids_an_existing_real_name() {
        // "모델-2" 라는 이름이 이미 있으면 겹친 뒤의 것은 다음 수 -3 을 받는다.
        // byte 문자열은 ASCII 만 되므로 한글 이름 있는 본문은 text 로 쓴다
        let body = r#"{"five_hour":{"utilization":1},"limits":[
            {"kind":"weekly_scoped","percent":1},
            {"kind":"weekly_scoped","percent":2,"scope":{"model":{"display_name":"모델-2"}}},
            {"kind":"weekly_scoped","percent":3}]}"#;
        let l = parse(body.as_bytes()).unwrap();
        assert_eq!(ids(&l), ["5h", "7d-모델", "7d-모델-2", "7d-모델-3"]);
        // -2 는 진짜 이름 "모델-2" 의 것
        assert_eq!(l[2].label, "주간 · 모델-2");
        assert_eq!(l[3].label, "주간 · 모델");
    }

    #[test]
    fn skipped_items_take_no_id() {
        // 값이 숫자가 아닌 항목은 그것만 빠지고 id 를 차지하지 않는다: 같은 이름의 앞 것이 빠지면 뒤의 것이 -2 없이
        let body = br#"{"five_hour":{"utilization":1},"limits":[
            {"kind":"weekly_scoped","percent":"x","scope":{"model":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}}]}"#;
        let l = parse(body).unwrap();
        assert_eq!(ids(&l), ["5h", "7d-Opus"]);
        assert_eq!(l[1].pct, 3.0);
    }

    #[test]
    fn five_hour_and_week_ids_never_change() {
        // 5h·7d 는 접미사를 붙이지 않는다: 모델별 주간에 겹침이 있어도 그대로
        let body = br#"{"five_hour":{"utilization":1},"seven_day":{"utilization":2},"limits":[
            {"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}},
            {"kind":"weekly_scoped","percent":4,"scope":{"model":{"display_name":"Opus"}}}]}"#;
        let l = parse(body).unwrap();
        assert_eq!(ids(&l), ["5h", "7d", "7d-Opus", "7d-Opus-2"]);
    }

    #[test]
    fn nothing_parsed_is_a_failure() {
        // 푼 한도가 하나도 없으면 실패다. 빈 "성공" 을 적용하고 캐시에 쓰면 보이던 한도가 사라지고
        // 상태줄도 그 빈 캐시를 믿어 버린다
        assert!(parse(br#"{"five_hour":{}}"#).is_none());
        assert!(parse(br#"{"five_hour":{"utilization":null}}"#).is_none());
    }

    #[test]
    fn five_hour_unparsed_but_week_ok_succeeds() {
        // 5시간을 못 풀어도 다른 한도(주간)가 풀리면 성공이다
        let body = br#"{"five_hour":{"utilization":"5"},"seven_day":{"utilization":13}}"#;
        let l = parse(body).unwrap();
        assert_eq!(ids(&l), ["7d"]);
        assert_eq!(l[0].pct, 13.0);
    }

    #[test]
    fn cache_and_generation() {
        let dir = std::env::temp_dir().join(format!("cb-usage-{}", crate::util::short_id()));
        fs::create_dir_all(&dir).unwrap();
        let mut u = Usage::new(dir.join(".claude-usage-cache.json"));
        let now = clock::now();
        // 캐시가 없으면 받는다, 받는 중에는 다시 받지 않는다
        let Next::Fetch(g) = u.refresh(now) else { panic!() };
        assert!(matches!(u.refresh(now), Next::Idle));
        assert!(u.finish(g, None, false));
        assert!(u.failed);
        let Next::Fetch(g) = u.refresh(now) else { panic!() };
        assert!(u.finish(g, Some(SAMPLE.as_bytes().to_vec()), false));
        assert!(!u.failed && u.limits.len() == 4 && u.cache.exists());
        // 막 쓴 캐시는 그대로 쓴다
        u.limits.clear();
        assert!(matches!(u.refresh(clock::now()), Next::Idle));
        assert_eq!(u.limits.len(), 4);
        // 계정을 바꾸면 캐시를 지우고, 그 전에 시작한 응답(세대 0)은 버린다
        u.reset(clock::now());
        assert!(!u.cache.exists());
        assert!(u.limits.is_empty());
        assert!(!u.finish(0, Some(SAMPLE.as_bytes().to_vec()), true));
        assert!(u.limits.is_empty() && !u.token_expired);
        // 바꾼 뒤 5초 안에 쓰인 캐시는 믿지 않는다
        fs::write(&u.cache, SAMPLE).unwrap();
        assert!(matches!(u.refresh(clock::now()), Next::Fetch(1)));
        assert!(u.limits.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn check_cache_pins_today() {
        let now = 1_000_000.0;
        let body = SAMPLE.as_bytes();
        // 캐시가 없으면 믿어 쓸 한도가 없어 바로 받는다
        assert_eq!(check_cache(None, None, false, now), (None, true));
        // 낡지 않은 캐시(10초)는 그대로 쓰고 받지 않는다
        assert_eq!(check_cache(Some(now - 10.0), Some(body), false, now), (parse(body), false));
        // 경계 넘을 때만 받는다, 나이가 정확히 120초면 아직 받지 않는다
        assert_eq!(check_cache(Some(now - 120.0), Some(body), false, now), (parse(body), false));
        // 낡은 캐시(121초)는 받는다
        assert_eq!(check_cache(Some(now - 121.0), Some(body), false, now), (parse(body), true));
        // 받고 있는 동안에는 낡아도 다시 받지 않는다
        assert_eq!(check_cache(Some(now - 121.0), Some(body), true, now), (parse(body), false));
    }

    #[test]
    fn untrusted_cache_is_neither_applied_nor_waited_for() {
        // 믿을 만하게 쓰인 수정 시각이 없으면(written None) 본문이 있어도 한도로 버리고 바로 받는다
        let now = 1_000_000.0;
        assert_eq!(check_cache(None, Some(SAMPLE.as_bytes() as &[u8]), false, now), (None, true));
    }

    #[test]
    fn unparsable_cache_body_fetches() {
        // 못 푸는(깨진) 캐시는 믿을 캐시가 없는 것과 같다: 나이를 세지 않고 바로 받는다.
        // 상태줄 스크립트가 같은 파일에 못 읽는 내용을 계속 써도 수정 시각에 막혀 120초씩 기다리지 않는다
        let now = 1_000_000.0;
        assert_eq!(check_cache(Some(now - 10.0), Some(b"{\"five_" as &[u8]), false, now), (None, true));
        // JSON 이지만 five_hour 가 없거나 푼 한도가 하나도 없어도 같다
        assert_eq!(check_cache(Some(now - 10.0), Some(br#"{"seven_day":{"utilization":1}}"# as &[u8]), false, now), (None, true));
        assert_eq!(check_cache(Some(now - 10.0), Some(br#"{"five_hour":{}}"# as &[u8]), false, now), (None, true));
        // 내용을 못 읽었을 때도 같다(그 사이 지워짐)
        assert_eq!(check_cache(Some(now - 10.0), None, false, now), (None, true));
    }

    #[test]
    fn future_cache_mtime_applies_and_fetches() {
        // 수정 시각이 미래(시계 어긋남)면 나이가 음수다: 그래도 적용하되(풀리기만 하면) 받는다
        let now = 1_000_000.0;
        assert_eq!(check_cache(Some(now + 60.0), Some(SAMPLE.as_bytes() as &[u8]), false, now), (parse(SAMPLE.as_bytes()), true));
        // 0.5 초 미래라도 받는다
        assert_eq!(check_cache(Some(now + 0.5), Some(SAMPLE.as_bytes() as &[u8]), false, now), (parse(SAMPLE.as_bytes()), true));
    }

    #[test]
    fn just_written_cache_does_not_fetch() {
        // 지금 막 쓴 캐시(나이 0)는 적용만 하고 받지 않는다
        let now = 1_000_000.0;
        assert_eq!(check_cache(Some(now), Some(SAMPLE.as_bytes() as &[u8]), false, now), (parse(SAMPLE.as_bytes()), false));
    }

    #[test]
    fn fetching_cache_never_fetches() {
        // 받는 중이면 믿을 캐시가 없든(없음·깨짐) 미래든 한참 낡았든 다시 받지 않는다. 적용은 그대로 한다
        let now = 1_000_000.0;
        assert_eq!(check_cache(Some(now + 60.0), Some(SAMPLE.as_bytes() as &[u8]), true, now), (parse(SAMPLE.as_bytes()), false));
        assert_eq!(check_cache(Some(now - 300.0), Some(SAMPLE.as_bytes() as &[u8]), true, now), (parse(SAMPLE.as_bytes()), false));
        assert_eq!(check_cache(Some(now - 10.0), Some(b"garbage" as &[u8]), true, now), (None, false));
        assert_eq!(check_cache(None, None, true, now), (None, false));
    }
}
