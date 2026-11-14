//! 계정 사용률 대체 길 — `Usage` 포트의 어댑터. stream 에 사용률이 없을 때만 부른다.
//! 차례: ① 보드·상태줄이 함께 쓰는 캐시 파일(15분 안) ② 사용량 API(로그인 토큰, `Http` 포트로).
//! 캐시 파일은 읽기만 한다 — API 로 받은 값을 거기에 쓰면 보드가 낡은 값을 새것으로 믿는다.
//! 토큰은 부를 때마다 파일에서 읽어 요청 헤더에만 싣고, 필드·반환값·오류 문구 어디에도 두지 않는다.
//! 원본: `usageFromCache`·`usageFromAPI`(`Sources/ClaudeAutopilot/main.swift`), 파일 꼴은 보드 `usage.rs` 와 같다.

use std::ffi::OsString;
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

/// 시험용 사용량 파일 경로. 값은 절대 경로. macOS 의 시험 빌드 설정 `usage_path`(`autopilotUsagePath`)에 해당한다.
pub const USAGE_PATH_ENV: &str = "CLAUDE_BOARD_USAGE_PATH";
/// 시험용 오프라인 표지. 값 `1` 이면 사용량 API·로그인 파일을 쓰지 않는다. macOS 의 시험 빌드 설정 `usageOffline` 에 해당한다.
pub const USAGE_OFFLINE_ENV: &str = "CLAUDE_BOARD_USAGE_OFFLINE";

/// 시험 게이트의 판정 결과. 둘 다 기본값(꺼짐)이면 설치판과 같다.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UsageOverride {
    /// 장난감 사용량 파일. 있으면 캐시·API 대신 이것만 읽는다(수정 시각의 나이를 따지지 않는다)
    pub toy_path: Option<PathBuf>,
    /// 사용량 API·로그인 파일을 건드리지 않는다(캐시는 읽기만)
    pub offline: bool,
}

impl UsageOverride {
    /// `CLAUDE_BOARD_HOME` 이 있을 때만 두 변수를 듣는다 — 사용자 환경에 남은 변수 하나가 진짜 사용량 조회를 바꾸지 못하게(SPEC §5.13 과 같은 게이트).
    /// 경로가 절대 경로가 아니면 무시한다(macOS 도 `/` 로 시작하는 경로만 받는다).
    pub fn from_env(env: &dyn Fn(&str) -> Option<OsString>) -> Self {
        let get = |key: &str| env(key).filter(|v| !v.is_empty());
        if get(claude_board_core::paths::HOME_ENV).is_none() {
            return Self::default();
        }
        let toy_path = get(USAGE_PATH_ENV).map(PathBuf::from).filter(|p| p.is_absolute());
        let offline = get(USAGE_OFFLINE_ENV).is_some_and(|v| v == "1");
        Self { toy_path, offline }
    }
}

pub struct FallbackUsage<H: Http> {
    http: H,
    credentials_path: PathBuf,
    cache_path: PathBuf,
    over: UsageOverride,
}

impl<H: Http> FallbackUsage<H> {
    pub fn new(http: H, credentials_path: PathBuf, cache_path: PathBuf) -> Self {
        Self { http, credentials_path, cache_path, over: UsageOverride::default() }
    }

    /// 시험 게이트(`UsageOverride`)를 건다.
    pub fn with_override(mut self, over: UsageOverride) -> Self {
        self.over = over;
        self
    }

    /// 이 PC 의 실제 경로: `~/.claude/.credentials.json` 과 `%TEMP%\.claude-usage-cache.json`(보드와 같은 파일).
    /// 시험 게이트 변수(`USAGE_PATH_ENV`·`USAGE_OFFLINE_ENV`)는 이 프로세스의 환경에서 읽는다.
    pub fn system(http: H) -> Self {
        Self::new(
            http,
            claude_board_core::paths::credentials_file(),
            std::env::temp_dir().join(".claude-usage-cache.json"),
        )
        .with_override(UsageOverride::from_env(&|k| std::env::var_os(k)))
    }

    /// 장난감 파일: 나이를 따지지 않고 수정 시각을 `at` 으로 읽는다(macOS `.toy` 걸음). 없거나 꼴이 틀리면 None.
    fn read_toy(path: &Path) -> Option<UsageSnapshot> {
        let written = fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
        snapshot_from_body(&fs::read(path).ok()?, UsageFrom::Toy, written)
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
        // 장난감 파일이 정해졌으면 캐시도 API 도 보지 않는다(macOS `apUsageSteps` 의 [toy, stream])
        if let Some(path) = &self.over.toy_path {
            return Ok(Self::read_toy(path));
        }
        let cached = self.read_cache(now);
        if self.over.offline {
            return Ok(cached);
        }
        Ok(cached.or_else(|| self.fetch_api(now)))
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
