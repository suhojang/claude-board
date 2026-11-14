// 버전 목록 어댑터(`adapters::versions`) 시험 — Swift 시험은 없고 이곳의 검증이 전부다.
// 가짜 Http(주소별 상태·본문, 부른 요청 수집)로 받기 꼴을, 임시 폴더로 캐시 라운드트리를,
// 둘을 이은 `load_catalog` 흐름 끝까지를 본다. 규칙 자체의 시험은 `rules_versions.rs` 가 맡는다.
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use claude_board_autopilot::adapters::http::UreqHttp;
use claude_board_autopilot::adapters::versions::{cache_path, feeds_enabled, CacheFile, HttpFeeds, VERSIONS_OFFLINE_ENV};
use claude_board_autopilot::ports::{Clock, Http, HttpRequest, HttpResponse, PortError, PortErrorKind, PortResult};
use claude_board_autopilot::rules::versions::{build_catalog, catalog, catalog_data, load_catalog, CatalogOrigin, Feed, VersionCache, VersionFeeds};

const FEED_TIMEOUT: &std::time::Duration = &std::time::Duration::from_secs(10);
const NOW: f64 = 1_800_000_000.0;

// ── 가짜 Http ─────────────────────────────────────────────────────────

/// 답하는 꼴
#[derive(Clone)]
enum Reply {
    /// 표에 없는 주소는 오류로 답한다
    Bodies(HashMap<String, (u16, Vec<u8>)>),
    /// 무엇을 부르든 이 상태(본문은 비었다)
    Status(u16),
    /// 무엇을 부르든 이 오류
    Error(PortErrorKind),
}

/// 부른 요청을 모으는 가짜 Http.
struct FakeHttp {
    reply: Reply,
    calls: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeHttp {
    fn status(status: u16) -> FakeHttp {
        FakeHttp { reply: Reply::Status(status), calls: Arc::default() }
    }
    fn bodies(bodies: HashMap<String, (u16, Vec<u8>)>) -> FakeHttp {
        FakeHttp { reply: Reply::Bodies(bodies), calls: Arc::default() }
    }
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Http for FakeHttp {
    fn send(&self, req: &HttpRequest) -> PortResult<HttpResponse> {
        self.calls.lock().unwrap().push(req.clone());
        match &self.reply {
            Reply::Bodies(bodies) => match bodies.get(&req.url) {
                Some((status, body)) => Ok(HttpResponse { status: *status, headers: vec![], body: body.clone() }),
                None => Err(PortError::new(PortErrorKind::Io, "표에 없는 주소")),
            },
            Reply::Status(status) => Ok(HttpResponse { status: *status, headers: vec![], body: Vec::new() }),
            Reply::Error(kind) => Err(PortError::new(*kind, "실패 이유는 쓰지 않는다")),
        }
    }
}

/// 공식 목록 응답표(모든 곳 200) — `rules_versions.rs` 의 `feeds_today` 표와 같은 값이다.
fn official_bodies() -> HashMap<Feed, Vec<u8>> {
    let m: &[(Feed, &[u8])] = &[
        (Feed::Spring, SPRING_META.as_bytes()),
        (Feed::Nest, br#"{"latest":"12.3.2","legacy":"11.2.8","next":"12.0.0-alpha.7"}"#),
        (Feed::React, br#"{"latest":"19.3.1","canary":"19.3.0-canary"}"#),
        (Feed::Next, br#"{"latest":"16.4.3"}"#),
        (Feed::Vue, br#"{"latest":"3.5.44"}"#),
        (Feed::Vite, br#"{"latest":"8.3.5"}"#),
        (Feed::Fastapi, br#"{"info":{"version":"0.162.0"}}"#),
    ];
    let mut g: HashMap<Feed, Vec<u8>> = m.iter().map(|(f, b)| (*f, b.to_vec())).collect();
    g.insert(Feed::Temurin, eol(TEMURIN, &[]));
    g.insert(Feed::SpringBoot, eol(SPRING_BOOT, SPRING_BOOT_JAVA));
    g.insert(Feed::Node, eol(NODE, &[]));
    g.insert(Feed::Python, eol(PYTHON, &[]));
    g.insert(Feed::Postgresql, eol(POSTGRESQL, &[]));
    g.insert(Feed::Mysql, eol(MYSQL, &[]));
    g.insert(Feed::Kotlin, eol(KOTLIN, &[]));
    g
}

/// 받는 Http 꼴 — 주소를 키로 하고 상태 200 을 함께 담는 표
fn feed_bodies() -> HashMap<String, (u16, Vec<u8>)> {
    official_bodies().iter().map(|(f, b)| (f.url().to_string(), (200u16, b.clone()))).collect()
}

/// `feed_bodies` 에서 상태를 뺀 본문표 — 규칙 `build_catalog` 인수 꼴
fn body_map() -> HashMap<Feed, Vec<u8>> {
    official_bodies().iter().map(|(f, b)| (*f, b.clone())).collect()
}

/// 공식 응답표 — 위 `feed_bodies` 의 값(`rules_versions.rs` 에서 옮긴 것)
const SPRING_META: &str = r##"
{"bootVersion":{"default":"4.1.2","values":[{"id":"4.2.0-SNAPSHOT"},{"id":"4.2.0-RC1"},{"id":"4.1.3-SNAPSHOT"},{"id":"4.1.2"},{"id":"4.0.10-SNAPSHOT"},{"id":"4.0.9"}]},
 "javaVersion":{"default":"17","values":[{"id":"27"},{"id":"25"},{"id":"21"},{"id":"17"}]}}
"##;
type RelRow = (&'static str, bool, bool, Option<&'static str>, Option<&'static str>);
const TEMURIN: &[RelRow] = &[
    ("27", false, false, Some("2027-03-23"), Some("27.0.1")),
    ("25", true, false, Some("2031-09-30"), Some("25.0.5")),
    ("21", true, false, Some("2029-12-31"), Some("21.0.13")),
];
const SPRING_BOOT: &[RelRow] = &[("4.1", false, false, Some("2027-07-31"), Some("4.1.2"))];
const SPRING_BOOT_JAVA: &[(&str, &str)] = &[("4.1", "17 - 26")];
const NODE: &[RelRow] = &[
    ("27", false, false, Some("2027-06-01"), Some("27.0.0")),
    ("26", true, false, Some("2029-04-30"), Some("26.13.0")),
];
const PYTHON: &[RelRow] = &[
    ("3.15", false, false, Some("2031-10-31"), Some("3.15.2")),
    ("3.14", false, false, Some("2030-10-31"), Some("3.14.10")),
];
const POSTGRESQL: &[RelRow] = &[
    ("18", false, false, Some("2030-11-14"), Some("18.6")),
    ("17", false, false, Some("2029-11-08"), Some("17.11")),
];
const MYSQL: &[RelRow] = &[
    ("26.10", false, false, None, Some("26.10.0")),
    ("9.7", true, false, Some("2034-04-30"), Some("9.7.4")),
];
const KOTLIN: &[RelRow] = &[("2.4", false, false, None, Some("2.4.21"))];

/// `eol(...)` 응답 글 만들기(`rules_versions.rs` 와 같다)
fn eol(rel: &[RelRow], java: &[(&str, &str)]) -> Vec<u8> {
    let r: Vec<serde_json::Value> = rel
        .iter()
        .map(|(n, lts, gone, from, latest)| {
            let mut o = serde_json::json!({"name": n, "isLts": lts, "isEol": gone, "eolFrom": from});
            if let Some(l) = latest {
                o["latest"] = serde_json::json!({"name": l});
            }
            if let Some((_, j)) = java.iter().find(|(k, _)| k == n) {
                o["custom"] = serde_json::json!({"supportedJavaVersions": j});
            }
            o
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({"schema_version": "1.2.0", "result": {"name": "x", "releases": r}})).unwrap()
}

/// `load_catalog` 를 어댑터로 끝까지 도리는 짝 — (가짜 Http, 캐시 파일, 가짜 시계)
struct FakeClock {
    now: f64,
}
impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.now
    }
    fn sleep(&self, _d: std::time::Duration) {}
}

// ── HttpFeeds ─────────────────────────────────────────────────────────

mod http_feeds_tests {
    use super::*;

    #[test]
    fn 표() {
        // 열네 곳 모두: GET·원본 주소·Accept 헤더·본문 없음·10초, 200 이면 본문을 돌려준다
        for feed in Feed::ALL {
            let http = FakeHttp::status(200);
            let feeds = HttpFeeds { http: &http };
            let got = feeds.fetch(feed).expect("200 은 본문이 있다");
            assert_eq!(got, Vec::<u8>::new());
            let calls = http.calls.lock().unwrap();
            assert_eq!(calls.len(), 1, "{}", feed.url());
            let r = &calls[0];
            assert_eq!(r.method, "GET");
            assert_eq!(r.url, feed.url());
            assert_eq!(r.headers, vec![("Accept".to_string(), feed.accept().to_string())]);
            assert_eq!(r.body, None);
            assert_eq!(r.timeout, *FEED_TIMEOUT);
            assert!(!feed.url().contains('?'), "사용자 자료가 아니므로 질의 없다");
        }
    }

    #[test]
    fn 상태_200이_아니면_받지_못했다() {
        // 원본 `statusCode == 200` 만 본문으로 친다 — 리다이렉트·오류 상태는 모두 없다
        for status in [204u16, 301, 304, 404, 500] {
            let http = FakeHttp::status(status);
            let feeds = HttpFeeds { http: &http };
            assert_eq!(feeds.fetch(Feed::Node), None, "{status}");
            assert_eq!(http.count(), 1, "{status}");
        }
    }

    #[test]
    fn 보내기_실패도_받지_못했다() {
        let http = FakeHttp { reply: Reply::Error(PortErrorKind::Unavailable), calls: Arc::default() };
        let feeds = HttpFeeds { http: &http };
        assert_eq!(feeds.fetch(Feed::Postgresql), None);
        assert_eq!(http.count(), 1);
    }

    #[test]
    fn 본문_상한_경계() {
        // 4 MiB 미만만 받는다 — 정확히 4 MiB 는 넘는 것으로 본다(원본 `d.count < 4 * 1024 * 1024`)
        let limit = 4 * 1024 * 1024;
        for (name, len, want) in [("4MiB - 1", limit - 1, true), ("4MiB", limit, false), ("빈 본문", 0, true)] {
            let feed = Feed::Python;
            let mut bodies = HashMap::new();
            let body = vec![b'{'; len];
            let want_body = body.clone();
            bodies.insert(feed.url().to_string(), (200, body));
            let feeds = HttpFeeds { http: &FakeHttp::bodies(bodies) };
            let got = feeds.fetch(feed);
            let note = name;
            assert_eq!(got.is_some(), want, "{note}");
            if want {
                assert_eq!(got.unwrap(), want_body, "{note}");
            }
        }
    }
}

// ── CacheFile ─────────────────────────────────────────────────────────

mod cache_file_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn 라운드트립_과_덮어쓰기() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let path = cache_path(&root);
        let cache = CacheFile { path: path.clone() };
        // 없을 때는 없다(규칙이 없는 캐시를 앱에 든 목록으로 되돌린다)
        assert_eq!(cache.read(), None);
        assert!(!root.join("cache").exists(), "읽기는 폴더를 만들지 않는다");
        // 처음 쓰면 부모 폴더도 만든다
        let first = b"{\"at\":1,\"items\":{},\"notes\":{}}".to_vec();
        cache.write(&first);
        assert!(root.join("cache").exists(), "cache 폴더를 만든다");
        assert!(path.is_file());
        // 읽으면 내용과 쓴 시각(유닉스 초)
        let (data, secs) = cache.read().expect("쓴 파일은 읽힌다");
        assert_eq!(data, first);
        let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert!(secs > wall - 60.0 && secs <= wall + 1.0, "mtime 은 방금 전이다: {secs} vs {wall}");
        // 다시 쓰면 덮는다
        let second = b"{\"at\":2,\"items\":{},\"notes\":{}}".to_vec();
        cache.write(&second);
        let (data, _) = cache.read().expect("덮은 파일도 읽힌다");
        assert_eq!(data, second, "덮어쓰기가 새 것으로 바꾼다");
        // 임시 파일이 남지 않는다
        let names: Vec<String> = cache_path(&root).parent().unwrap().read_dir().unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(!names.iter().any(|n| n.contains(".tmp-")), "임시 파일을 지웠다: {names:?}");
    }

    #[test]
    fn 쓰기_실패_임시_파일_뒷정리() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 자리가 폴더이면 이름 바꾸기가 실패한다(파일로 폴더를 덮지 못한다) — 그 자리에 써서 임시 파일을 지우는지 본다
        fs::create_dir(root.join("target")).unwrap();
        let target = root.join("target");
        let cache = CacheFile { path: target.clone() };
        cache.write(b"x");
        assert!(target.is_dir(), "대상 폴더는 그대로");
        let names: Vec<String> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["target".to_string()], "실패하면 임시 파일을 지운다");
        // 폴더 자리는 낡은 캐시로도 읽지 못한다
        assert_eq!(cache.read(), None);
    }

    #[test]
    fn 없는_자리는_없다() {
        // 부모 폴더까지 없어도 읽기가 없다고만 말한다
        let dir = tempfile::tempdir().unwrap();
        let cache = CacheFile { path: dir.path().join("cache").join("stack-versions.json") };
        assert_eq!(cache.read(), None);
        assert!(!Path::new(&cache.path).exists());
    }
}

// ── load_catalog 끝까지(가짜 Http + 진짜 캐시 파일) ────────────────────

mod load_catalog_tests {
    use super::*;

    fn loaded(clock: &FakeClock, feeds: &HttpFeeds, root: &std::path::Path) -> claude_board_autopilot::rules::versions::CatalogLoad {
        let cache = CacheFile { path: cache_path(root) };
        load_catalog(Some(feeds), &cache, clock, b"").expect("캐시 앱 목록이 비어도 열네 곳은 다 쓴다")
    }

    /// 지금(실제 시계, 초) — CacheFile 의 `at` 은 파일 고친 때(실제 시각)이므로, 캐시 나이를 보는 가짜 시계도 실제 시각을 기준으로 둔다
    fn wall() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64()
    }

    #[test]
    fn 끝까지_받아_쓰고_다시_읽는다() {
        // 1) 모든 곳이 200 — Fresh 로 만들어 진짜 캐시 파일에 쓴다
        let http = FakeHttp::bodies(feed_bodies());
        let feeds = HttpFeeds { http: &http };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let l = loaded(&FakeClock { now: NOW }, &feeds, &root);
        assert_eq!(l.origin, CatalogOrigin::Fresh);
        assert!(l.failed.is_empty());
        let want = build_catalog(&body_map(), NOW).unwrap();
        assert_eq!(l.catalog, want);
        // 받은 요청은 열네 곳 전부, 모두 GET
        assert_eq!(http.count(), 14);
        for r in http.calls.lock().unwrap().iter() {
            assert_eq!(r.method, "GET");
        }
        // 2) 캐시 파일에 공식 목록 글이 쓰였다(글은 규칙 `catalog` 로 다시 풀린다)
        let written = fs::read(cache_path(&root)).expect("캐시 파일이 생겼다");
        assert_eq!(catalog(&written), Some(l.catalog.clone()), "쓴 캐시는 규칙으로 다시 풀린다");
        assert_eq!(written, catalog_data(&l.catalog));
        // 3) 하루 안에 다시 부르면 파일에서 읽는다 — Http 를 부르지 않는다(캐시 판단 기준은 파일 고친 때라 실제 시각에 맞춘다)
        let later = FakeHttp::status(500);
        let later_feeds = HttpFeeds { http: &later };
        let l2 = loaded(&FakeClock { now: wall() + 60.0 }, &later_feeds, &root);
        assert_eq!(l2.origin, CatalogOrigin::Cached);
        assert_eq!(l2.catalog, l.catalog);
        assert!(l2.failed.is_empty());
        assert_eq!(later.count(), 0, "하루 안의 캐시는 Http 를 타지 않는다");
        // 4) 하루가 지나고 모든 곳이 500 — 잃어버리지 않고 낡은 캐시를 쓴다
        let older = FakeHttp::status(500);
        let older_feeds = HttpFeeds { http: &older };
        let l3 = loaded(&FakeClock { now: wall() + 86_400.0 + 60.0 }, &older_feeds, &root);
        assert_eq!(l3.origin, CatalogOrigin::Stale);
        assert_eq!(l3.catalog, l.catalog, "낡은 캐시 본문은 파일에서 다시 읽은 것");
        assert!(older.count() > 0, "하루가 지나면 다시 받는다");
        // 모든 곳이 상태 500 — 실패한 곳 이름(원본 표기)
        assert_eq!(l3.failed, ["start.spring.io", "endoflife.date", "npm", "PyPI"].iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}

// ── 끄기(시험 빌드의 오프라인 스위치) ─────────────────────────────────

/// 한 줄 — (시험 빌드인가, 환경 목록, 받기가 켜져야 하는가)
type SwitchRow = (bool, Vec<(String, String)>, bool);

#[test]
fn feeds_enabled_표() {
    // 시험 빌드에서 CLAUDE_BOARD_VERSIONS_OFFLINE=1 일 때만 끈다(원본 UserDefaults autopilotVersionsOffline)
    let var = |v: &str| vec![(VERSIONS_OFFLINE_ENV.to_string(), v.to_string())];
    let rows: [SwitchRow; 7] = [
        (true, Vec::new(), true),
        (true, var("1"), false),
        (true, var("0"), true),
        (true, var("1 "), true),
        (false, var("1"), true),
        (true, vec![("OTHER".to_string(), "1".to_string())], true),
        (true, vec![("CLAUDE_BOARD_USAGE_OFFLINE".to_string(), "1".to_string()), (VERSIONS_OFFLINE_ENV.to_string(), "1".to_string())], false),
    ];
    for (i, (dev, env, want)) in rows.iter().enumerate() {
        assert_eq!(feeds_enabled(*dev, env), *want, "행 {i}");
    }
    assert_eq!(VERSIONS_OFFLINE_ENV, "CLAUDE_BOARD_VERSIONS_OFFLINE");
}

// ── 진짜 호출(모두 --ignored) ─────────────────────────────────────────

/// 모든 공식 받을 곳을 진짜로 부른다 — `cargo test -p claude-board-autopilot --test versions_adapter -- --ignored`
#[test]
#[ignore = "네트워크가 필요하다 — 열네 곳 각각 최대 10초"]
fn 실제_호출로_모든_공식_곳_받기() {
    let http = UreqHttp::new();
    let feeds = HttpFeeds { http: &http };
    let mut got = 0usize;
    for feed in Feed::ALL {
        if let Some(body) = feeds.fetch(feed) {
            assert!(!body.is_empty(), "{} 빈 본문", feed.url());
            assert!(body.len() < 4 * 1024 * 1024, "{} 본문이 상한을 넘는다", feed.url());
            eprintln!("{} → {} 바이트", feed.raw_value(), body.len());
            got += 1;
        }
    }
    assert!(got > 0, "네 곳도 받지 못했다 — 네트워크 상태를 본다");
    eprintln!("{got}/14 받음");
}
