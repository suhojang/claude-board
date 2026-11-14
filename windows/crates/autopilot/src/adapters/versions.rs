//! 버전 목록 어댑터 — 규칙 `rules::versions` 의 받기(`VersionFeeds`)와 하루 캐시(`VersionCache`) 포트의 진짜 구현.
//! 공식 버전 목록은 고정한 공개 주소(`rules::versions::Feed`)만 GET 한다 — 이 파일이 주소를 더할 길은 없다.
//! 끔(오래된 목록 화면을 네트워크 없이 보려고)은 시험 빌드의 환경 값으로 한다(`feeds_enabled`).
//! 원본: `APHTTPFeeds`·`APCatalogCacheFile`(`Sources/ClaudeBoard/AutopilotTopic.swift` 88~115, 오프라인 스위치 512 줄).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::ports::{Http, HttpRequest};
use crate::rules::versions::{Feed, VersionCache, VersionFeeds};

/// 시험 빌드만: 받기를 끄는 환경 값. 값이 정확히 "1" 일 때만 듣는다.
/// macOS 의 설정 `autopilotVersionsOffline`(UserDefaults)에 해당한다.
pub const VERSIONS_OFFLINE_ENV: &str = "CLAUDE_BOARD_VERSIONS_OFFLINE";

/// 요청 시간 상한 — 원본 세션의 `timeoutIntervalForRequest` 10 초
const FEEDS_TIMEOUT: Duration = Duration::from_secs(10);
/// 본문 상한(바이트) — 미만일 때만 받은 것으로 친다. 원본 `d.count < 4 * 1024 * 1024`
const MAX_FEED_BODY: usize = 4 * 1024 * 1024;

/// 공식 버전 목록 받기 — 고정한 공개 주소만 인증 없이 GET 하고, 사용자 자료는 실어 보내지 않는다.
/// 200 이고 본문이 4 MiB 미만일 때만 본문을 돌려준다. 그 밖의 상태·오류·파일 꼴은 모두 "받지 못했다"(None).
/// 원본: `APHTTPFeeds`(쿠키·캐시 없는 세션, 10초, 200 이고 4MB 아래일 때만)
pub struct HttpFeeds<'a> {
    pub http: &'a dyn Http,
}

impl VersionFeeds for HttpFeeds<'_> {
    fn fetch(&self, feed: Feed) -> Option<Vec<u8>> {
        let req = HttpRequest {
            method: "GET".to_string(),
            url: feed.url().to_string(),
            headers: vec![("Accept".to_string(), feed.accept().to_string())],
            body: None,
            timeout: FEEDS_TIMEOUT,
        };
        // 원본은 실패 이유를 보지 않으므로 오류 문구도 어디에도 남지 않는다
        let resp = self.http.send(&req).ok()?;
        if resp.status != 200 {
            return None;
        }
        (resp.body.len() < MAX_FEED_BODY).then_some(resp.body)
    }
}

/// 받은 목록의 하루 캐시 파일 자리 — `<자율주행 뿌리>/cache/stack-versions.json`(보드만 쓴다). 원본: `APCatalogCacheFile.path`
pub fn cache_path(autopilot_root: &Path) -> PathBuf {
    autopilot_root.join("cache").join("stack-versions.json")
}

/// 받은 목록의 하루 캐시 — 내용과 쓴 시각(유닉스 초). 쓰는 규칙은 원본 세 값(`try?`·`unlink`)을 그대로 접는다:
/// 부모 폴더를 만들고, `<경로>.tmp-<pid>` 에 써서 원래 자리로 이름을 바꾸고, 실패하면 임시 파일을 지운다.
/// 그 밖의 실패는 소리 없이 버린다(캐시가 없는 것). 원본: `APCatalogCacheFile`
pub struct CacheFile {
    pub path: PathBuf,
}

impl VersionCache for CacheFile {
    fn read(&self) -> Option<(Vec<u8>, f64)> {
        let data = fs::read(&self.path).ok()?;
        let secs = fs::metadata(&self.path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs_f64();
        Some((data, secs))
    }

    fn write(&self, data: &[u8]) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = pending_path(&self.path);
        let ok = fs::write(&tmp, data).is_ok() && fs::rename(&tmp, &self.path).is_ok();
        if !ok {
            let _ = fs::remove_file(&tmp);
        }
    }
}

/// 쓰다 만 캐시의 자리(`<경로>.tmp-<pid>`) — 같은 파일을 여러 보드가 동시에 써도 충돌하지 않게 pid 를 붙인다
fn pending_path(path: &Path) -> PathBuf {
    let mut with_pending = path.as_os_str().to_os_string();
    with_pending.push(format!(".tmp-{}", std::process::id()));
    PathBuf::from(with_pending)
}

/// 받기를 켤지 말지 — 시험 빌드(`dev`)에서 `VERSIONS_OFFLINE_ENV=1` 이면 끈다. 그 밖에는 늘 켠다.
/// 원본: `loadCatalog` 의 스위치 `apDevBuild && UserDefaults.standard.bool(forKey: "autopilotVersionsOffline")`
pub fn feeds_enabled(dev: bool, env: &[(String, String)]) -> bool {
    !(dev && env.iter().any(|(k, v)| k == VERSIONS_OFFLINE_ENV && v == "1"))
}
