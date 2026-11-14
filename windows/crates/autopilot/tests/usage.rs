// 계정 사용률 대체 어댑터 시험 — 가짜 Http 와 임시 파일로 한다.
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use claude_board_autopilot::adapters::usage::{FallbackUsage, UsageOverride, USAGE_OFFLINE_ENV, USAGE_PATH_ENV};
use claude_board_autopilot::ports::*;

const NOW: f64 = 1_800_000_000.0;
const TOKEN: &str = "sk-ant-oat01-SECRET-TOKEN-VALUE";
const BODY: &str = r#"{
  "five_hour": {"utilization": 84.0, "resets_at": "2027-01-15T10:00:00+00:00"},
  "seven_day": {"utilization": 31, "resets_at": "2027-01-18T00:00:00Z"}
}"#;

#[derive(Clone)]
enum Reply {
    Ok(u16, String),
    Fail(PortErrorKind),
}

/// 부른 요청을 모으는 가짜 Http.
#[derive(Clone)]
struct FakeHttp {
    reply: Reply,
    calls: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeHttp {
    fn new(reply: Reply) -> Self {
        Self { reply, calls: Arc::default() }
    }
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Http for FakeHttp {
    fn send(&self, req: &HttpRequest) -> PortResult<HttpResponse> {
        self.calls.lock().unwrap().push(req.clone());
        match &self.reply {
            Reply::Ok(status, body) => Ok(HttpResponse { status: *status, headers: vec![], body: body.clone().into_bytes() }),
            // 일부러 토큰이 든 문구를 돌려줘서 어댑터가 오류 문구를 흘리지 않는지 본다
            Reply::Fail(kind) => Err(PortError::new(*kind, format!("failed with {TOKEN}"))),
        }
    }
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap() }
    }
    fn cache(&self) -> PathBuf {
        self.dir.path().join(".claude-usage-cache.json")
    }
    fn creds(&self) -> PathBuf {
        self.dir.path().join(".credentials.json")
    }
    fn write_cache(&self, body: &str, age_secs: f64) {
        fs::write(self.cache(), body).unwrap();
        set_mtime(&self.cache(), NOW - age_secs);
    }
    fn write_creds(&self, expires_at_ms: f64) {
        let j = format!(r#"{{"claudeAiOauth":{{"accessToken":"{TOKEN}","expiresAt":{expires_at_ms}}}}}"#);
        fs::write(self.creds(), j).unwrap();
    }
    fn usage(&self, http: &FakeHttp) -> FallbackUsage<FakeHttp> {
        FallbackUsage::new(http.clone(), self.creds(), self.cache())
    }
}

fn set_mtime(path: &Path, unix_secs: f64) {
    let f = File::options().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs_f64(unix_secs)).unwrap();
}

fn valid_expiry() -> f64 {
    (NOW + 3600.0) * 1000.0
}

#[test]
fn fresh_cache_wins_and_http_is_never_called() {
    let env = Env::new();
    env.write_cache(BODY, 60.0);
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
    assert_eq!(http.count(), 0);
    assert_eq!(s.from, UsageFrom::Cache);
    assert!((s.five_hour - 0.84).abs() < 1e-9);
    assert!((s.seven_day - 0.31).abs() < 1e-9);
    assert!((s.at - (NOW - 60.0)).abs() < 1e-3, "at 은 캐시 수정 시각");
    assert_eq!(s.five_hour_resets, Some(1_800_007_200.0));
    assert_eq!(s.seven_day_resets, Some(1_800_230_400.0));
}

#[test]
fn cache_age_boundary_is_fifteen_minutes() {
    let env = Env::new();
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Fail(PortErrorKind::Io));
    env.write_cache(BODY, 899.0);
    assert_eq!(env.usage(&http).fallback(NOW).unwrap().unwrap().from, UsageFrom::Cache);
    env.write_cache(BODY, 901.0);
    assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None);
    assert_eq!(http.count(), 1);
}

#[test]
fn stale_cache_falls_to_api_and_does_not_write_the_cache() {
    let env = Env::new();
    env.write_cache(r#"{"five_hour":{"utilization":1},"seven_day":{"utilization":2}}"#, 3600.0);
    env.write_creds(valid_expiry());
    let before = fs::read(env.cache()).unwrap();
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
    assert_eq!(http.count(), 1);
    assert_eq!(s.from, UsageFrom::Api);
    assert_eq!(s.at, NOW);
    assert!((s.five_hour - 0.84).abs() < 1e-9);
    assert_eq!(fs::read(env.cache()).unwrap(), before, "API 결과를 공유 캐시에 쓰지 않는다");
}

#[test]
fn api_request_shape() {
    let env = Env::new();
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    env.usage(&http).fallback(NOW).unwrap().unwrap();
    let calls = http.calls.lock().unwrap();
    let req = &calls[0];
    assert_eq!((req.method.as_str(), req.url.as_str()), ("GET", "https://api.anthropic.com/api/oauth/usage"));
    assert!(req.headers.contains(&("Authorization".into(), format!("Bearer {TOKEN}"))));
    assert!(req.headers.contains(&("anthropic-beta".into(), "oauth-2025-04-20".into())));
    assert!(req.body.is_none());
}

#[test]
fn broken_or_empty_cache_falls_to_api() {
    for broken in ["{\"five_", "", "[]", r#"{"five_hour":{"utilization":1}}"#, r#"{"seven_day":{"utilization":"x"}}"#] {
        let env = Env::new();
        env.write_cache(broken, 10.0);
        env.write_creds(valid_expiry());
        let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
        let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
        assert_eq!(http.count(), 1, "깨진 캐시: {broken:?}");
        assert_eq!(s.from, UsageFrom::Api);
    }
}

#[test]
fn future_cache_mtime_is_not_trusted() {
    let env = Env::new();
    env.write_cache(BODY, -120.0);
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    assert_eq!(env.usage(&http).fallback(NOW).unwrap().unwrap().from, UsageFrom::Api);
}

#[test]
fn api_failures_give_none() {
    let replies = [
        Reply::Ok(401, "{}".into()),
        Reply::Ok(500, "oops".into()),
        Reply::Ok(200, "not json".into()),
        Reply::Fail(PortErrorKind::Timeout),
        Reply::Fail(PortErrorKind::Io),
    ];
    for reply in replies {
        let env = Env::new();
        env.write_creds(valid_expiry());
        let http = FakeHttp::new(reply);
        assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None);
        assert_eq!(http.count(), 1);
    }
}

#[test]
fn missing_broken_or_expired_credentials_give_none_without_calling_http() {
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    // 파일 없음
    let env = Env::new();
    assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None);
    // 깨진 파일, 토큰 없음
    for bad in ["not json", "{}", r#"{"claudeAiOauth":{}}"#] {
        fs::write(env.creds(), bad).unwrap();
        assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None, "{bad:?}");
    }
    // 만료
    env.write_creds((NOW - 1.0) * 1000.0);
    assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None);
    assert_eq!(http.count(), 0);
}

#[test]
fn null_five_hour_is_zero_and_weekly_is_required() {
    // 5시간 한도 없는 요금제
    for body in [
        r#"{"five_hour":null,"seven_day":{"utilization":40,"resets_at":"2027-01-18T00:00:00Z"}}"#,
        r#"{"seven_day":{"utilization":40,"resets_at":"2027-01-18T00:00:00Z"}}"#,
        r#"{"five_hour":{"utilization":null},"seven_day":{"utilization":40,"resets_at":"2027-01-18T00:00:00Z"}}"#,
    ] {
        let env = Env::new();
        env.write_cache(body, 5.0);
        let http = FakeHttp::new(Reply::Fail(PortErrorKind::Io));
        let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
        assert_eq!((s.five_hour, s.five_hour_resets), (0.0, None), "{body}");
        assert!((s.seven_day - 0.4).abs() < 1e-9);
        assert!(s.seven_day_resets.is_some());
    }
    // 주간이 없거나 null 이면 못 안다
    for body in [r#"{"five_hour":{"utilization":10}}"#, r#"{"five_hour":{"utilization":10},"seven_day":null}"#] {
        let env = Env::new();
        env.write_creds(valid_expiry());
        let http = FakeHttp::new(Reply::Ok(200, body.into()));
        assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None, "{body}");
    }
}

/// 5시간 한도가 없는 요금제(Ultra)도 멈출 선 판정이 주간 선만 본다(맥 `apUsageFromPercent` 5시간은 있을 때만과 같은 판정).
#[test]
fn null_five_hour_plan_is_judged_by_the_weekly_stopline_only() {
    use claude_board_autopilot::rules::stopline::{stopline_wait, Tz};
    use serde_json::{json, Map, Value};
    let line: Map<String, Value> = json!({"seven_day": 0.8, "five_hour": 0.95}).as_object().unwrap().clone();
    for (weekly, waits) in [(26, false), (85, true)] {
        let env = Env::new();
        let body = format!(r#"{{"five_hour":null,"seven_day":{{"utilization":{weekly},"resets_at":"2027-01-18T00:00:00Z"}}}}"#);
        env.write_cache(&body, 5.0);
        let http = FakeHttp::new(Reply::Fail(PortErrorKind::Io));
        let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
        let usage: Map<String, Value> = json!({
            "five_hour": s.five_hour, "seven_day": s.seven_day,
            "seven_day_resets": s.seven_day_resets, "five_hour_resets": s.five_hour_resets,
        })
        .as_object()
        .unwrap()
        .clone();
        let wait = stopline_wait(Some(&usage), &line, NOW, Tz::Utc);
        assert_eq!(wait.is_some(), waits, "주간 {weekly}%");
        if let Some(w) = wait {
            assert!(w.reason.contains("주간 사용률 85%"), "{}", w.reason);
        }
    }
}

#[test]
fn malformed_five_hour_is_not_read_as_zero() {
    let env = Env::new();
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, r#"{"five_hour":{"utilization":"84"},"seven_day":{"utilization":3}}"#.into()));
    assert_eq!(env.usage(&http).fallback(NOW).unwrap(), None);
}

#[test]
fn over_hundred_percent_is_clamped_to_one() {
    let env = Env::new();
    env.write_cache(r#"{"five_hour":{"utilization":130},"seven_day":{"utilization":-5}}"#, 1.0);
    let http = FakeHttp::new(Reply::Fail(PortErrorKind::Io));
    let s = env.usage(&http).fallback(NOW).unwrap().unwrap();
    assert_eq!((s.five_hour, s.seven_day), (1.0, 0.0));
}

#[test]
fn token_never_appears_in_returned_values_or_errors() {
    let replies = [
        Reply::Ok(200, BODY.into()),
        Reply::Ok(401, format!("bad token {TOKEN}")),
        Reply::Fail(PortErrorKind::Timeout),
        Reply::Fail(PortErrorKind::Denied),
    ];
    for reply in replies {
        let env = Env::new();
        env.write_creds(valid_expiry());
        let http = FakeHttp::new(reply);
        let u = env.usage(&http);
        let shown = match u.fallback(NOW) {
            Ok(v) => format!("{v:?}"),
            Err(e) => format!("{e:?} {e}"),
        };
        assert!(!shown.contains(TOKEN), "{shown}");
    }
}

// ---- 시험 게이트: CLAUDE_BOARD_USAGE_PATH · CLAUDE_BOARD_USAGE_OFFLINE (CLAUDE_BOARD_HOME 이 있을 때만) ----

fn fake_env(pairs: &[(&str, String)]) -> impl Fn(&str) -> Option<OsString> {
    let map: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    move |key| map.iter().find(|(k, _)| k == key).map(|(_, v)| OsString::from(v))
}

#[test]
fn gate_ignores_both_variables_without_home() {
    let toy = Env::new().dir.path().join("toy.json").to_string_lossy().into_owned();
    let env = fake_env(&[(USAGE_PATH_ENV, toy), (USAGE_OFFLINE_ENV, "1".into())]);
    assert_eq!(UsageOverride::from_env(&env), UsageOverride::default());
    // 빈 홈도 없는 것과 같다
    let env = fake_env(&[("CLAUDE_BOARD_HOME", String::new()), (USAGE_OFFLINE_ENV, "1".into())]);
    assert_eq!(UsageOverride::from_env(&env), UsageOverride::default());
}

#[test]
fn gate_reads_both_variables_with_home() {
    let toy = Env::new().dir.path().join("toy.json");
    let env = fake_env(&[
        ("CLAUDE_BOARD_HOME", "x".into()),
        (USAGE_PATH_ENV, toy.to_string_lossy().into_owned()),
        (USAGE_OFFLINE_ENV, "1".into()),
    ]);
    assert_eq!(UsageOverride::from_env(&env), UsageOverride { toy_path: Some(toy), offline: true });
}

#[test]
fn gate_rejects_relative_path_and_non_one_offline() {
    let env = fake_env(&[
        ("CLAUDE_BOARD_HOME", "x".into()),
        (USAGE_PATH_ENV, "relative/toy.json".into()),
        (USAGE_OFFLINE_ENV, "true".into()),
    ]);
    assert_eq!(UsageOverride::from_env(&env), UsageOverride::default());
}

#[test]
fn toy_file_replaces_cache_and_api_and_ignores_age() {
    let env = Env::new();
    // 캐시는 신선하고 값이 다르다 — 장난감이 이겨야 한다
    env.write_cache(r#"{"five_hour":{"utilization":1},"seven_day":{"utilization":2}}"#, 10.0);
    env.write_creds(valid_expiry());
    let toy = env.dir.path().join("toy.json");
    fs::write(&toy, BODY).unwrap();
    set_mtime(&toy, NOW - 100_000.0); // 하루 넘게 묵어도 읽는다
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let u = env.usage(&http).with_override(UsageOverride { toy_path: Some(toy), offline: false });
    let s = u.fallback(NOW).unwrap().unwrap();
    assert_eq!(s.from, UsageFrom::Toy);
    assert!((s.five_hour - 0.84).abs() < 1e-9 && (s.seven_day - 0.31).abs() < 1e-9);
    assert!((s.at - (NOW - 100_000.0)).abs() < 1e-3, "at 은 장난감 파일 수정 시각");
    assert_eq!(http.count(), 0);
}

#[test]
fn missing_or_broken_toy_file_gives_none_without_cache_or_api() {
    let env = Env::new();
    env.write_cache(BODY, 10.0);
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let toy = env.dir.path().join("toy.json");
    let over = || UsageOverride { toy_path: Some(toy.clone()), offline: false };
    assert_eq!(env.usage(&http).with_override(over()).fallback(NOW).unwrap(), None);
    fs::write(&toy, "not json").unwrap();
    assert_eq!(env.usage(&http).with_override(over()).fallback(NOW).unwrap(), None);
    assert_eq!(http.count(), 0);
}

#[test]
fn offline_reads_fresh_cache_but_never_calls_api_or_reads_credentials() {
    let env = Env::new();
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let off = || UsageOverride { toy_path: None, offline: true };
    // 캐시 없음·낡음 → 모른다(API 로 떨어지지 않는다)
    assert_eq!(env.usage(&http).with_override(off()).fallback(NOW).unwrap(), None);
    env.write_cache(BODY, 901.0);
    assert_eq!(env.usage(&http).with_override(off()).fallback(NOW).unwrap(), None);
    assert_eq!(http.count(), 0);
    // 신선한 캐시는 읽는다
    env.write_cache(BODY, 60.0);
    assert_eq!(env.usage(&http).with_override(off()).fallback(NOW).unwrap().unwrap().from, UsageFrom::Cache);
    assert_eq!(http.count(), 0);
}

#[test]
fn default_override_still_falls_to_api() {
    let env = Env::new();
    env.write_creds(valid_expiry());
    let http = FakeHttp::new(Reply::Ok(200, BODY.into()));
    let s = env.usage(&http).with_override(UsageOverride::default()).fallback(NOW).unwrap().unwrap();
    assert_eq!((s.from, http.count()), (UsageFrom::Api, 1));
}
