//! 요금제 사용 한도 — `UsageStore` (`ClaudeBoard.swift:262-354`, SPEC §5.4).
//! 30초마다 본다. 상태줄 스크립트와 같은 캐시 파일을 함께 써서 API 호출을 줄인다. 토큰은 메모리에만 두고 기록하지 않는다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use claude_board_core::clock::iso_date;
use claude_board_core::{clock, fileio, Record};
use serde_json::Value;

use crate::model::{Limit, UsageView};
use crate::util::{http_get, ms, mtime, num, obj};

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
        if written.is_some() {
            if let Some(limits) = fs::read(&self.cache).ok().and_then(|b| parse(&b)) {
                self.limits = limits;
            }
        }
        let age = now - written.unwrap_or(f64::NEG_INFINITY);
        if age <= 120.0 || self.fetching {
            return Next::Idle;
        }
        self.fetching = true;
        Next::Fetch(self.generation)
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

/// 사용량 API 응답을 한도 목록으로 — 5시간·주간 전체·주간 모델별. `five_hour` 가 없으면 None.
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
}
