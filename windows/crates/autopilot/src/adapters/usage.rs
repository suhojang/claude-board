//! 계정 사용률 대체 길 — `Usage` 포트의 어댑터. stream 에 사용률이 없을 때만 부른다.
//! 차례: ① 보드·상태줄이 함께 쓰는 캐시 파일(15분 안) ② 사용량 API(로그인 토큰, `Http` 포트로).
//! 캐시 파일은 읽기만 한다 — API 로 받은 값을 거기에 쓰면 보드가 낡은 값을 새것으로 믿는다.
//! 토큰은 부를 때마다 파일에서 읽어 요청 헤더에만 싣고, 필드·반환값·오류 문구 어디에도 두지 않는다.
//! 원본: `usageFromCache`·`usageFromAPI`(`Sources/ClaudeAutopilot/main.swift`), 파일 꼴은 보드 `usage.rs` 와 같다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use claude_board_core::clock::iso_date;
use serde_json::{Map, Value};

use crate::ports::{Http, HttpRequest, PortResult, Usage, UsageFrom, UsageSnapshot};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// 캐시를 믿는 나이(초) — macOS `usageFromCache` 의 900
const CACHE_MAX_AGE_SECS: f64 = 900.0;
const API_TIMEOUT: Duration = Duration::from_secs(10);

pub struct FallbackUsage<H: Http> {
    http: H,
    credentials_path: PathBuf,
    cache_path: PathBuf,
}

impl<H: Http> FallbackUsage<H> {
    pub fn new(http: H, credentials_path: PathBuf, cache_path: PathBuf) -> Self {
        Self { http, credentials_path, cache_path }
    }

    /// 이 PC 의 실제 경로: `~/.claude/.credentials.json` 과 `%TEMP%\.claude-usage-cache.json`(보드와 같은 파일).
    pub fn system(http: H) -> Self {
        Self::new(
            http,
            claude_board_core::paths::credentials_file(),
            std::env::temp_dir().join(".claude-usage-cache.json"),
        )
    }

    fn read_cache(&self, now: f64) -> Option<UsageSnapshot> {
        let written = fs::metadata(&self.cache_path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
        let age = now - written;
        // 미래 수정 시각(시계 어긋남)은 믿지 않는다. 보드도 같은 경우를 낡은 것으로 본다
        if !(0.0..CACHE_MAX_AGE_SECS).contains(&age) {
            return None;
        }
        let body = fs::read(&self.cache_path).ok()?;
        snapshot_from_body(&body, UsageFrom::Cache, written)
    }

    fn fetch_api(&self, now: f64) -> Option<UsageSnapshot> {
        let token = read_token(&self.credentials_path, now)?;
        let req = HttpRequest {
            method: "GET".into(),
            url: USAGE_URL.into(),
            headers: vec![
                ("Authorization".into(), format!("Bearer {token}")),
                ("anthropic-beta".into(), "oauth-2025-04-20".into()),
            ],
            body: None,
            timeout: API_TIMEOUT,
        };
        // 어떤 실패든 "모른다" 로 접는다. 오류 문구에 요청 내용이 섞여 올라오지 않게 버린다
        let resp = self.http.send(&req).ok()?;
        if resp.status != 200 {
            return None;
        }
        snapshot_from_body(&resp.body, UsageFrom::Api, now)
    }
}

impl<H: Http> Usage for FallbackUsage<H> {
    fn fallback(&self, now: f64) -> PortResult<Option<UsageSnapshot>> {
        Ok(self.read_cache(now).or_else(|| self.fetch_api(now)))
    }
}

/// 로그인 파일에서 접근 토큰을 꺼낸다. 없거나 만료됐으면 None(만료된 토큰으로는 부르지 않는다).
fn read_token(path: &Path, now: f64) -> Option<String> {
    let Value::Object(rec) = serde_json::from_slice(&fs::read(path).ok()?).ok()? else { return None };
    let oauth = rec.get("claudeAiOauth")?.as_object()?;
    let token = oauth.get("accessToken")?.as_str()?;
    if oauth.get("expiresAt").and_then(Value::as_f64).is_some_and(|ms| ms / 1000.0 < now) {
        return None;
    }
    Some(token.to_string())
}

/// 캐시·API 가 같은 꼴이다: `{five_hour:{utilization 84.0, resets_at}, seven_day:{…}}`, 퍼센트를 0~1 로 맞춘다.
/// 주간 값은 필수이고 없으면 None. 5시간 한도가 없는 요금제는 `five_hour` 가 null 이라
/// 5시간은 0.0 이고 초기화 시각은 None 이다(한도가 없으니 5시간 때문에 멈추지 않는다).
fn snapshot_from_body(body: &[u8], from: UsageFrom, at: f64) -> Option<UsageSnapshot> {
    let Value::Object(j) = serde_json::from_slice(body).ok()? else { return None };
    let (seven_day, seven_day_resets) = window(j.get("seven_day")?.as_object()?)?;
    // 한도 없음(null·빠짐·utilization null)만 0.0 이고, 꼴이 틀린 값은 낮게 읽지 않고 통째로 버린다
    let (five_hour, five_hour_resets) = match j.get("five_hour") {
        None | Some(Value::Null) => (0.0, None),
        Some(Value::Object(five)) if matches!(five.get("utilization"), None | Some(Value::Null)) => (0.0, None),
        Some(Value::Object(five)) => window(five)?,
        Some(_) => return None,
    };
    Some(UsageSnapshot { five_hour, five_hour_resets, seven_day, seven_day_resets, at, from })
}

/// (사용 비율 0~1, 초기화 시각). `utilization` 이 숫자가 아니면 None.
fn window(w: &Map<String, Value>) -> Option<(f64, Option<f64>)> {
    let pct = w.get("utilization")?.as_f64()?;
    let resets = w.get("resets_at").and_then(Value::as_str).and_then(iso_date);
    Some(((pct / 100.0).clamp(0.0, 1.0), resets))
}
