//! 기술 스택 버전 목록 규칙(`rules::versions`) 시험. 원본: `Tests/AutopilotRulesTests/VersionsTests.swift`(Swift 시험 6개 + 표 인수 전부 옮김).
//! 옮긴 시험(같은 이름): `CatalogTests` 의 `표`·`메모`·`실패`·`앱목록`·`주소` 와 `CatalogLoadTests` 의 `표`·`머리`. 이름이 겹치는 `표` 둘은 모듈(`catalog_tests`·`catalog_load_tests`)로 가른다.
//! `extra_` 시험은 Swift 시험에 없는 것이다 — 풀기·고르기·범위 읽기·캐시 글 꼴·하루 경계·받지 못한 곳 이름·받기 꺼짐을 Swift 시험이 만든 응답(`springMeta`·`feedsToday`)과 경계값으로 본다.
//! 원본에서 `private` 인 `majorMinor`·`month`·`num`·`inRange` 는 공개하지 않으므로 목록(`build_catalog`·`eol_list`)을 거쳐 본다.
//! 표(`TEMURIN`·`SPRING_BOOT`·`NODE`·`PYTHON`·`POSTGRESQL`·`MYSQL`·`KOTLIN`·`SPRING_BOOT_JAVA`·`CATALOG_ROWS`·`LOAD_ROWS`)은
//! Swift 파일에서 변환 스크립트로 기계가 옮긴 것이다(손으로 다시 치지 않았다). 앱에 든 목록 글은 `rules::builtin_versions::BUILTIN_CATALOG_JSON` 을 쓴다.
//! `extra_eol_list_표` 의 기대 글은 Swift `catalogRows` 의 `want` 에서 `eol_list` 가 만드는 항목(node·python·postgresql·mysql·kotlin, java 는 Temurin 줄 가운데 LTS 만 고른 것)을 그대로 옮긴 것이다.

use claude_board_autopilot::ports::Clock;
use claude_board_autopilot::rules::builtin_versions::BUILTIN_CATALOG_JSON;
use claude_board_autopilot::rules::versions::{
    build_catalog, catalog, catalog_data, catalog_label, eol_list, load_catalog, parse_eol, parse_npm_tags, parse_pypi, parse_spring, Catalog,
    CatalogLoad, CatalogOrigin, EolRelease, Feed, VersionCache, VersionFeeds, Ver,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

type Row = (&'static str, bool, bool, Option<&'static str>, Option<&'static str>);

const TEMURIN: &[Row] = &[
    ("27", false, false, Some("2027-03-23"), Some("27.0.1")),
    ("25", true, false, Some("2031-09-30"), Some("25.0.5")),
    ("21", true, false, Some("2029-12-31"), Some("21.0.13")),
    ("17", true, false, Some("2027-10-31"), Some("17.0.21")),
    ("11", true, false, Some("2027-10-31"), Some("11.0.32")),
    ("8", true, false, Some("2030-12-31"), Some("8u504")),
];
const SPRING_BOOT: &[Row] = &[
    ("4.1", false, false, Some("2027-07-31"), Some("4.1.2")),
    ("4.0", false, false, Some("2026-12-31"), Some("4.0.9")),
    ("3.5", false, true, Some("2026-06-30"), Some("3.5.9")),
];
const SPRING_BOOT_JAVA: &[(&str, &str)] = &[("4.1", "17 - 26"), ("4.0", "17 - 25")];
const NODE: &[Row] = &[
    ("27", false, false, Some("2027-06-01"), Some("27.0.0")),
    ("26", true, false, Some("2029-04-30"), Some("26.13.0")),
    ("24", true, false, Some("2028-04-30"), Some("24.22.2")),
    ("22", true, false, Some("2027-04-30"), Some("22.23.4")),
    ("20", true, true, Some("2026-04-30"), Some("20.19.0")),
];
const PYTHON: &[Row] = &[
    ("3.15", false, false, Some("2031-10-31"), Some("3.15.2")),
    ("3.14", false, false, Some("2030-10-31"), Some("3.14.10")),
    ("3.13", false, false, Some("2029-10-31"), Some("3.13.18")),
    ("3.12", false, false, Some("2028-10-31"), Some("3.12.16")),
];
const POSTGRESQL: &[Row] = &[
    ("18", false, false, Some("2030-11-14"), Some("18.6")),
    ("17", false, false, Some("2029-11-08"), Some("17.11")),
    ("16", false, false, Some("2028-11-09"), Some("16.15")),
    ("15", false, false, Some("2027-11-11"), Some("15.19")),
];
const MYSQL: &[Row] = &[
    ("26.10", false, false, None, Some("26.10.0")),
    ("9.7", true, false, Some("2034-04-30"), Some("9.7.4")),
    ("8.4", true, false, Some("2032-04-30"), Some("8.4.13")),
];
const KOTLIN: &[Row] = &[
    ("2.4", false, false, None, Some("2.4.21")),
];

/// Swift `eol(...)` — 응답 글을 만든다(`eolFrom` 이 없으면 `null`)
fn eol(rel: &[Row], java: &[(&str, &str)]) -> Vec<u8> {
    let r: Vec<Value> = rel
        .iter()
        .map(|(n, lts, gone, from, latest)| {
            let mut o = json!({"name": n, "isLts": lts, "isEol": gone, "eolFrom": from});
            if let Some(l) = latest {
                o["latest"] = json!({"name": l});
            }
            if let Some((_, j)) = java.iter().find(|(k, _)| k == n) {
                o["custom"] = json!({"supportedJavaVersions": j});
            }
            o
        })
        .collect();
    serde_json::to_vec(&json!({"schema_version": "1.2.0", "result": {"name": "x", "releases": r}})).unwrap()
}

fn releases(rel: &[Row]) -> Vec<EolRelease> {
    parse_eol(&eol(rel, &[])).unwrap()
}

/// Swift `show`
fn show(l: &[Ver]) -> String {
    l.iter().map(|x| format!("{}{}{}", x.v, if x.lts { " LTS" } else { "" }, if x.rec { " 추천" } else { "" })).collect::<Vec<_>>().join(", ")
}

const SPRING_META: &str = r##"
{"bootVersion":{"default":"4.1.2","values":[{"id":"4.2.0-SNAPSHOT"},{"id":"4.2.0-RC1"},{"id":"4.1.3-SNAPSHOT"},{"id":"4.1.2"},{"id":"4.0.10-SNAPSHOT"},{"id":"4.0.9"}]},
 "javaVersion":{"default":"17","values":[{"id":"27"},{"id":"25"},{"id":"21"},{"id":"17"}]}}
"##;

// ── 옮긴 Swift 시험 ─────────────────────────────────

#[test]
fn 주소() {
    // 받을 곳은 고정한 공개 주소뿐(https, 사용자 자료 없음)
    for f in Feed::ALL {
        let u = f.url();
        assert!(
            u.starts_with("https://start.spring.io/")
                || u.starts_with("https://endoflife.date/api/v1/products/")
                || u.starts_with("https://registry.npmjs.org/-/package/")
                || u.starts_with("https://pypi.org/pypi/"),
            "{u}"
        );
        assert!(!u.contains('?'), "{u}");
    }
    assert_eq!(Feed::Spring.accept(), "application/vnd.initializr.v2.3+json");
}

// ── 이 절반의 추가 시험 ─────────────────────────────────

#[test]
fn extra_받을_곳_목록과_이름() {
    assert_eq!(Feed::ALL.len(), 14);
    let raws: Vec<&str> = Feed::ALL.iter().map(|f| f.raw_value()).collect();
    assert_eq!(
        raws,
        ["spring", "temurin", "springBoot", "node", "python", "postgresql", "mysql", "kotlin", "nest", "react", "next", "vue", "vite", "fastapi"]
    );
    let mut urls: Vec<&str> = Feed::ALL.iter().map(|f| f.url()).collect();
    urls.sort_unstable();
    urls.dedup();
    assert_eq!(urls.len(), 14, "주소는 서로 다르다");
    assert_eq!(Feed::Fastapi.url(), "https://pypi.org/pypi/fastapi/json");
    assert_eq!(Feed::Nest.url(), "https://registry.npmjs.org/-/package/@nestjs/core/dist-tags");
    for f in Feed::ALL {
        if f != Feed::Spring {
            assert_eq!(f.accept(), "application/json");
        }
    }
    // 받지 못한 곳 이름
    assert_eq!(Feed::Spring.label(), "start.spring.io");
    for f in [Feed::Nest, Feed::React, Feed::Next, Feed::Vue, Feed::Vite] {
        assert_eq!(f.label(), "npm");
    }
    assert_eq!(Feed::Fastapi.label(), "PyPI");
    for f in [Feed::Temurin, Feed::SpringBoot, Feed::Node, Feed::Python, Feed::Postgresql, Feed::Mysql, Feed::Kotlin] {
        assert_eq!(f.label(), "endoflife.date");
    }
}

#[test]
fn extra_봄_응답_풀기() {
    let m = parse_spring(SPRING_META.as_bytes()).unwrap();
    assert_eq!(m.boots, ["4.1.2", "4.0.9"]);
    assert_eq!(m.boot_default.as_deref(), Some("4.1.2"));
    assert_eq!(m.javas, ["27", "25", "21", "17"]);
    // GA 없음
    let none = r#"{"bootVersion":{"values":[{"id":"4.2.0-SNAPSHOT"}]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(none.as_bytes()), None);
    // Java 판 없음
    let nojava = r#"{"bootVersion":{"values":[{"id":"4.1.2"}]},"javaVersion":{"values":[]}}"#;
    assert_eq!(parse_spring(nojava.as_bytes()), None);
    // 기본값이 GA 가 아니면 비운다 · 글자가 아니면 비운다
    let rc = r#"{"bootVersion":{"default":"4.2.0-RC1","values":[{"id":"4.1.2"}]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(rc.as_bytes()).unwrap().boot_default, None);
    let numd = r#"{"bootVersion":{"default":4,"values":[{"id":"4.1.2"}]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(numd.as_bytes()).unwrap().boot_default, None);
    // id 가 글자가 아닌 원소는 건너뛴다. 원소 하나가 객체가 아니면 values 통째로 빈 목록 → None
    let skip = r#"{"bootVersion":{"values":[{"id":1},{"x":"y"},{"id":"4.1.2"}]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(skip.as_bytes()).unwrap().boots, ["4.1.2"]);
    let mixed = r#"{"bootVersion":{"values":[{"id":"4.1.2"},"4.0.9"]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(mixed.as_bytes()), None);
    // "-" 가 든 판은 GA 가 아니다(…-M1·…-RC1), SNAPSHOT 글자가 든 판도 뺀다
    let ms = r#"{"bootVersion":{"values":[{"id":"4.2.0-M1"},{"id":"4.1.SNAPSHOT"},{"id":"4.1.2"}]},"javaVersion":{"values":[{"id":"25"}]}}"#;
    assert_eq!(parse_spring(ms.as_bytes()).unwrap().boots, ["4.1.2"]);
    // 칸이 없거나 객체가 아니면 None
    for bad in ["", "<html>", "[]", "\"x\"", "{}", r#"{"bootVersion":1,"javaVersion":{}}"#, r#"{"bootVersion":{"values":[{"id":"4.1.2"}]}}"#] {
        assert_eq!(parse_spring(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn extra_지원_끝_응답_풀기() {
    let t = parse_eol(&eol(TEMURIN, &[])).unwrap();
    assert_eq!(t.len(), 6);
    assert_eq!(t[0], EolRelease::new("27", false, false, Some("2027-03-23"), Some("27.0.1"), None));
    assert_eq!(t[5], EolRelease::new("8", true, false, Some("2030-12-31"), Some("8u504"), None));
    let sb = parse_eol(&eol(SPRING_BOOT, SPRING_BOOT_JAVA)).unwrap();
    assert_eq!(sb[0], EolRelease::new("4.1", false, false, Some("2027-07-31"), Some("4.1.2"), Some("17 - 26")));
    assert_eq!(sb[1].supported_java.as_deref(), Some("17 - 25"));
    assert!(sb[2].eol && sb[2].supported_java.is_none());
    // eolFrom 이 null 이면 None
    let k = parse_eol(&eol(KOTLIN, &[])).unwrap();
    assert_eq!(k[0], EolRelease::new("2.4", false, false, None, Some("2.4.21"), None));
    // 모양이 다르면 None
    for bad in [
        "",
        "<html>",
        "[]",
        "{}",
        r#"{"result":{}}"#,
        r#"{"result":{"releases":[]}}"#,
        r#"{"result":{"releases":"x"}}"#,
        r#"{"result":{"releases":[{"isLts":true}]}}"#,   // name 없음 → 걸러져 빈 목록
        r#"{"result":{"releases":[{"name":"1"},"x"]}}"#, // 객체가 아닌 원소 → 통째로 실패
        r#"{"result":{"releases":[{"name":1}]}}"#,       // name 이 글자가 아님
    ] {
        assert_eq!(parse_eol(bad.as_bytes()), None, "{bad}");
    }
    // name 없는 줄은 건너뛰고 나머지는 남긴다. 칸이 없거나 글자가 아니면 기본값
    let doc = r#"{"result":{"releases":[{"isLts":true},{"name":"9","isLts":1,"isEol":0,"latest":{"name":3},"custom":{"supportedJavaVersions":8}},{"name":"10","isLts":2,"isEol":"yes"}]}}"#;
    let r = parse_eol(doc.as_bytes()).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0], EolRelease::new("9", true, false, None, None, None));
    assert_eq!(r[1], EolRelease::new("10", false, false, None, None, None));
}

#[test]
fn extra_npm_과_pypi_풀기() {
    let nest = parse_npm_tags(br#"{"latest":"12.3.2","legacy":"11.2.8","next":"12.0.0-alpha.7"}"#).unwrap();
    assert_eq!(nest.len(), 3);
    assert_eq!(nest["latest"], "12.3.2");
    assert_eq!(nest["legacy"], "11.2.8");
    // 글자가 아닌 값은 뺀다
    let t = parse_npm_tags(br#"{"latest":"1.0.0","n":5,"z":null,"a":["x"]}"#).unwrap();
    assert_eq!(t.keys().collect::<Vec<_>>(), ["latest"]);
    // latest 가 없거나 비었거나 글자가 아니면 None
    for bad in [r#"{"next":"1.0.0"}"#, r#"{"latest":""}"#, r#"{"latest":3}"#, "[]", "<html>", ""] {
        assert_eq!(parse_npm_tags(bad.as_bytes()), None, "{bad}");
    }
    assert_eq!(parse_pypi(br#"{"info":{"version":"0.162.0"}}"#).as_deref(), Some("0.162.0"));
    for bad in [r#"{"info":{}}"#, r#"{"info":{"version":1}}"#, r#"{"info":"x"}"#, "{}", "[]", "<html>"] {
        assert_eq!(parse_pypi(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn extra_eol_list_표() {
    let want: [(&[Row], usize, &str); 5] = [
        (NODE, 3, "26 LTS 추천, 24 LTS, 22 LTS"),
        (PYTHON, 3, "3.15 추천, 3.14, 3.13"),
        (POSTGRESQL, 3, "18 추천, 17, 16"),
        (MYSQL, 2, "9.7 LTS 추천, 8.4 LTS"),
        (KOTLIN, 2, "2.4 추천"),
    ];
    for (rows, limit, w) in want {
        assert_eq!(show(&eol_list(&releases(rows), limit)), w);
    }
    // Temurin: 지원 중인 LTS 만(27 은 LTS 아님)
    assert_eq!(show(&eol_list(&releases(TEMURIN), 3)), "25 LTS 추천, 21 LTS, 17 LTS");
    // 지원 끝 달
    let node = eol_list(&releases(NODE), 3);
    assert_eq!(node[0].until.as_deref(), Some("2029-04"));
    let kotlin = eol_list(&releases(KOTLIN), 2);
    assert_eq!(kotlin[0].until, None);
    // 한도·빈 목록·모두 지원 끝
    assert_eq!(eol_list(&releases(NODE), 1).len(), 1);
    assert_eq!(eol_list(&releases(NODE), 0), Vec::<Ver>::new());
    assert_eq!(eol_list(&[], 3), Vec::<Ver>::new());
    let all_eol: Vec<EolRelease> = vec![EolRelease::new("1", true, true, None, None, None)];
    assert_eq!(eol_list(&all_eol, 3), Vec::<Ver>::new());
    assert_eq!(eol_list(&releases(PYTHON), 10).len(), 4);
}

#[test]
fn extra_목록과_추천_그리고_글_꼴() {
    let mut items = BTreeMap::new();
    items.insert("java".to_string(), vec![Ver::new("21", true, None, false), Ver::new("25", true, Some("2031-09"), true)]);
    items.insert("node".to_string(), vec![Ver::new("22", true, None, false), Ver::new("24", true, None, false)]);
    items.insert("empty".to_string(), vec![]);
    let c = Catalog::new(1.5, items, BTreeMap::new());
    assert_eq!(c.list("java").len(), 2);
    assert!(c.list("nope").is_empty());
    assert_eq!(c.recommended("java"), Some("25"));
    assert_eq!(c.recommended("node"), Some("22")); // 추천 표시가 없으면 맨 앞
    assert_eq!(c.recommended("empty"), None);
    assert_eq!(c.recommended("nope"), None);
    // 글 꼴: until 이 없으면 키를 뺀다
    let v = serde_json::to_value(Ver::new("21", true, None, false)).unwrap();
    assert_eq!(v, json!({"v": "21", "lts": true, "rec": false}));
    let v = serde_json::to_value(Ver::new("25", true, Some("2031-09"), true)).unwrap();
    assert_eq!(v, json!({"v": "25", "lts": true, "until": "2031-09", "rec": true}));
    // 되읽기. 기본값이 있어도 lts·rec 키가 없으면 풀지 못한다(Swift 합성 Decodable)
    let back: Ver = serde_json::from_value(json!({"v": "25", "lts": true, "rec": true})).unwrap();
    assert_eq!(back, Ver::new("25", true, None, true));
    assert!(serde_json::from_value::<Ver>(json!({"v": "25", "rec": true})).is_err());
    assert!(serde_json::from_value::<Ver>(json!({"v": "25", "lts": true})).is_err());
    let cat = serde_json::to_vec(&c).unwrap();
    assert_eq!(serde_json::from_slice::<Catalog>(&cat).unwrap(), c);
    assert!(serde_json::from_value::<Catalog>(json!({"at": 1.0, "items": {}})).is_err()); // notes 없음
}

// ── 목록 만들기·불러오기(원본 119~230줄) ─────────────────────────────────

/// Swift `feedsToday`
fn feeds_today() -> HashMap<Feed, Vec<u8>> {
    let mut g: HashMap<Feed, Vec<u8>> = HashMap::new();
    g.insert(Feed::Spring, SPRING_META.as_bytes().to_vec());
    g.insert(Feed::Temurin, eol(TEMURIN, &[]));
    g.insert(Feed::SpringBoot, eol(SPRING_BOOT, SPRING_BOOT_JAVA));
    g.insert(Feed::Node, eol(NODE, &[]));
    g.insert(Feed::Python, eol(PYTHON, &[]));
    g.insert(Feed::Postgresql, eol(POSTGRESQL, &[]));
    g.insert(Feed::Mysql, eol(MYSQL, &[]));
    g.insert(Feed::Kotlin, eol(KOTLIN, &[]));
    g.insert(Feed::Nest, br#"{"latest":"12.3.2","legacy":"11.2.8","next":"12.0.0-alpha.7"}"#.to_vec());
    g.insert(Feed::React, br#"{"latest":"19.3.1","canary":"19.3.0-canary"}"#.to_vec());
    g.insert(Feed::Next, br#"{"latest":"16.4.3"}"#.to_vec());
    g.insert(Feed::Vue, br#"{"latest":"3.5.44"}"#.to_vec());
    g.insert(Feed::Vite, br#"{"latest":"8.3.5"}"#.to_vec());
    g.insert(Feed::Fastapi, br#"{"info":{"version":"0.162.0"}}"#.to_vec());
    g
}

fn built(g: &HashMap<Feed, Vec<u8>>) -> Option<Catalog> {
    build_catalog(g, 1.0)
}

mod catalog_tests {
    use super::*;

    #[test]
    fn 표() {
        // 공식 응답으로 만든 목록과 추천
        let c = build_catalog(&feeds_today(), 1.0).unwrap();
        for (note, key, want) in CATALOG_ROWS {
            assert_eq!(show(c.list(key)), want, "{note}");
        }
    }

    #[test]
    fn 메모() {
        // 지원 끝 달과 Java 메모
        let c = build_catalog(&feeds_today(), 1.0).unwrap();
        assert_eq!(c.list("java").first().and_then(|v| v.until.as_deref()), Some("2031-09"));
        assert_eq!(c.list("spring-boot").last().and_then(|v| v.until.as_deref()), Some("2026-12"));
        assert_eq!(c.notes.get("java").map(String::as_str), Some("목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 26 밖)"));
        assert!(c.recommended("java") == Some("25") && c.recommended("nope").is_none());
    }

    #[test]
    fn 실패() {
        // 응답 하나라도 없거나 꼴이 다르면 만들지 않는다
        let mut g = feeds_today();
        g.remove(&Feed::Vue);
        assert!(build_catalog(&g, 1.0).is_none());
        g = feeds_today();
        // GA 없음
        g.insert(Feed::Spring, br#"{"bootVersion":{"values":[{"id":"4.2.0-SNAPSHOT"}]},"javaVersion":{"values":[{"id":"25"}]}}"#.to_vec());
        assert!(build_catalog(&g, 1.0).is_none());
        g = feeds_today();
        g.insert(Feed::Temurin, b"<html>".to_vec());
        assert!(build_catalog(&g, 1.0).is_none());
        g = feeds_today();
        // latest 없음
        g.insert(Feed::Nest, br#"{"next":"1.0.0"}"#.to_vec());
        assert!(build_catalog(&g, 1.0).is_none());
    }

    #[test]
    fn 앱목록() {
        // 앱에 든 목록은 풀리고 같은 꼴이다
        let c = catalog(BUILTIN_CATALOG_JSON.as_bytes());
        assert!(c.is_some());
        let c = c.unwrap();
        for k in ["java", "spring-boot", "kotlin", "node", "nest", "python", "fastapi", "react", "vite", "next", "vue", "postgresql", "mysql"] {
            assert!(!c.list(k).is_empty(), "항목 {k}");
            assert_eq!(c.list(k).iter().filter(|v| v.rec).count(), 1, "추천 하나 {k}");
        }
    }
}

// ── 이 절반의 추가 시험 ─────────────────────────────────

/// 봄 응답 글(`default` 가 `None` 이면 칸을 뺀다)
fn spring_meta(default: Option<&str>, boots: &[&str], javas: &[&str]) -> Vec<u8> {
    let ids = |v: &[&str]| v.iter().map(|x| format!(r#"{{"id":"{x}"}}"#)).collect::<Vec<_>>().join(",");
    let d = default.map_or(String::new(), |d| format!(r#""default":"{d}","#));
    format!(r#"{{"bootVersion":{{{d}"values":[{}]}},"javaVersion":{{"values":[{}]}}}}"#, ids(boots), ids(javas)).into_bytes()
}

fn sb_range(range: Option<&str>) -> HashMap<Feed, Vec<u8>> {
    let mut g = feeds_today();
    let java: Vec<(&str, &str)> = range.map(|r| ("4.1", r)).into_iter().collect();
    g.insert(Feed::SpringBoot, eol(SPRING_BOOT, &java));
    g
}

#[test]
fn extra_java_범위_안인가() {
    // 범위 글을 읽는 갈래(원본 inRange). 결과는 Java 목록과 메모로 본다
    let java = |range: Option<&str>| built(&sb_range(range)).map(|c| (show(c.list("java")), c.notes.get("java").cloned()));
    let all = "25 LTS 추천, 21 LTS, 17 LTS";
    let note27 = |r: &str| Some(format!("목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.1 의 범위 {r} 밖)"));
    assert_eq!(java(Some("17 - 26")), Some((all.to_string(), note27("17 - 26"))));
    assert_eq!(java(None), Some((all.to_string(), note27("모름"))));
    // 위쪽 끝은 포함(25 <= 25), 아래쪽 끝도 포함(25 >= 25)
    assert_eq!(java(Some("17 - 25")), Some((all.to_string(), note27("17 - 25"))));
    assert_eq!(java(Some("25 - 30")), Some(("25 LTS 추천".to_string(), Some("목록에 없는 판: Java 27·21·17(LTS 가 아니거나 Spring Boot 4.1 의 범위 25 - 30 밖)".to_string()))));
    assert_eq!(java(Some("17 - 20")), Some(("17 LTS 추천".to_string(), Some("목록에 없는 판: Java 27·25·21(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 20 밖)".to_string()))));
    // 공백 없이도, 탭·가로 공백 글자로 둘러싸도 읽는다
    assert_eq!(java(Some("25-30")).unwrap().0, "25 LTS 추천");
    assert_eq!(java(Some("\t25\t-\t30\t")).unwrap().0, "25 LTS 추천");
    assert_eq!(java(Some("\u{a0}25\u{a0}-\u{a0}30\u{a0}")).unwrap().0, "25 LTS 추천");
    assert_eq!(java(Some("\u{3000}25\u{2003}-\u{200a}30\u{202f}\u{205f}\u{1680}")).unwrap().0, "25 LTS 추천");
    // 줄바꿈은 공백이 아니라 마디가 수로 읽히지 않아 0 이 된다(원본 trimmingCharacters(.whitespaces))
    assert_eq!(java(Some("\n25 - 30")).unwrap().0, all);
    // 마디가 둘이 아니면 범위 없음과 같다(빈 조각은 버린다)
    for r in ["abc", "17", "-17", "17 - 26 - 30", "", "-"] {
        assert_eq!(java(Some(r)).unwrap().0, all, "{r:?}");
    }
    // 공백만 든 마디는 빈 조각이 아니라 0 이다(0 - 0 범위라 아무 판도 안 든다)
    assert_eq!(java(Some(" - ")), None);
    // 마디가 수가 아니면 0
    assert_eq!(java(Some("x - 30")).unwrap().0, all);
    assert_eq!(java(Some("17 - x")), None);
    assert_eq!(java(Some("26 - 17")), None);
    assert_eq!(java(Some("26 - 30")), None);
    // Java 판이 수가 아니면 범위 밖일 수 없다
    let mut g = sb_range(Some("26 - 30"));
    g.insert(Feed::Spring, spring_meta(Some("4.1.2"), &["4.1.2"], &["x"]));
    g.insert(Feed::Temurin, eol(&[("x", true, false, Some("2030-01-01"), None)], &[]));
    let c = built(&g).unwrap();
    assert_eq!(show(c.list("java")), "x LTS 추천");
    assert!(!c.notes.contains_key("java"));
}

#[test]
fn extra_스프링_부트_고르기() {
    let sb4: &[Row] = &[
        ("4.3", false, false, Some("2028-01-31"), None),
        ("4.2", false, false, Some("2027-12-31"), None),
        ("4.1", false, false, Some("2027-07-31"), None),
        ("4.0", false, false, Some("2026-12-31"), None),
        ("3.5", false, true, Some("2026-06-30"), None),
    ];
    let with = |default: Option<&str>, boots: &[&str], sb: &[Row], java: &[(&str, &str)]| {
        let mut g = feeds_today();
        g.insert(Feed::Spring, spring_meta(default, boots, &["27", "25", "21", "17"]));
        g.insert(Feed::SpringBoot, eol(sb, java));
        built(&g)
    };
    let range = [("4.1", "17 - 26"), ("4.0", "17 - 25")];
    // 같은 줄(4.1)은 하나만, 지원 끝났거나 endoflife 에 없는 줄은 뺀다
    let c = with(Some("4.1.2"), &["9.0.0", "4.1.2", "4.1.1", "3.5.9", "4.0.9"], sb4, &range).unwrap();
    assert_eq!(show(c.list("spring-boot")), "4.1.2 추천, 4.0.9");
    assert_eq!(c.list("spring-boot")[0].until.as_deref(), Some("2027-07"));
    assert_eq!(c.list("spring-boot")[1].until.as_deref(), Some("2026-12"));
    // 기본값이 목록에 없거나 비면 맨 앞이 추천
    for d in [None, Some("9.9.9"), Some("4.1.1")] {
        let c = with(d, &["4.1.2", "4.0.9"], sb4, &range).unwrap();
        assert_eq!(show(c.list("spring-boot")), "4.1.2 추천, 4.0.9", "{d:?}");
    }
    // 기본값이 두 번째 줄이면 그 줄이 추천이고, Java 범위도 그 줄에서 가져온다
    let c = with(Some("4.0.9"), &["4.1.2", "4.0.9"], sb4, &range).unwrap();
    assert_eq!(show(c.list("spring-boot")), "4.1.2, 4.0.9 추천");
    assert_eq!(c.notes["java"], "목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.0 의 범위 17 - 25 밖)");
    // 세 줄까지만 보인다(추천이 네 번째면 보이는 목록에는 추천 표시가 없다 — 원본 그대로)
    let c = with(Some("4.0.9"), &["4.3.0", "4.2.0", "4.1.2", "4.0.9"], sb4, &range).unwrap();
    assert_eq!(show(c.list("spring-boot")), "4.3.0, 4.2.0, 4.1.2");
    let c = with(Some("4.3.0"), &["4.3.0", "4.2.0", "4.1.2", "4.0.9"], sb4, &range).unwrap();
    assert_eq!(show(c.list("spring-boot")), "4.3.0 추천, 4.2.0, 4.1.2");
    // 지원 중인 줄이 하나도 없으면 만들지 않는다
    assert!(with(Some("3.5.9"), &["3.5.9"], sb4, &range).is_none());
    assert!(with(Some("9.0.0"), &["9.0.0"], sb4, &range).is_none());
}

#[test]
fn extra_java_고르기() {
    let with = |javas: &[&str], tem: &[Row], java: &[(&str, &str)]| {
        let mut g = feeds_today();
        g.insert(Feed::Spring, spring_meta(Some("4.1.2"), &["4.1.2"], javas));
        g.insert(Feed::Temurin, eol(tem, &[]));
        g.insert(Feed::SpringBoot, eol(SPRING_BOOT, java));
        built(&g)
    };
    // 넷이어도 셋까지만 보이고, 남은 판이 없으면 메모도 없다(보이는 목록이 아니라 전체 목록과 견준다)
    let tem4: &[Row] = &[
        ("25", true, false, Some("2031-09-30"), None),
        ("21", true, false, Some("2029-12-31"), None),
        ("17", true, false, Some("2027-10-31"), None),
        ("11", true, false, Some("2027-10-31"), None),
    ];
    let c = with(&["25", "21", "17", "11"], tem4, &[]).unwrap();
    assert_eq!(show(c.list("java")), "25 LTS 추천, 21 LTS, 17 LTS");
    assert!(!c.notes.contains_key("java"));
    // 지원 끝난 LTS 는 빼고 메모에 올린다(start.spring.io 차례대로 · 로 잇는다)
    let tem_eol: &[Row] = &[
        ("25", true, false, Some("2031-09-30"), None),
        ("21", true, true, Some("2026-01-01"), None),
        ("17", true, false, Some("2027-10-31"), None),
    ];
    let c = with(&["27", "25", "21", "17"], tem_eol, &[("4.1", "17 - 26")]).unwrap();
    assert_eq!(show(c.list("java")), "25 LTS 추천, 17 LTS");
    assert_eq!(c.notes["java"], "목록에 없는 판: Java 27·21(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 26 밖)");
    // LTS 가 아닌 판은 start.spring.io 에 있어도 뺀다 · 판 이름이 같은 줄이 없으면 뺀다
    let tem_non: &[Row] = &[("25", false, false, Some("2031-09-30"), None), ("21", true, false, Some("2029-12-31"), None)];
    let c = with(&["25", "21", "99"], tem_non, &[]).unwrap();
    assert_eq!(show(c.list("java")), "21 LTS 추천");
    assert_eq!(c.notes["java"], "목록에 없는 판: Java 25·99(LTS 가 아니거나 Spring Boot 4.1 의 범위 모름 밖)");
    // 하나도 남지 않으면 만들지 않는다
    assert!(with(&["25"], &[("25", false, false, None, None)], &[]).is_none());
    assert!(with(&["25"], &[("21", true, false, None, None)], &[]).is_none());
    assert!(with(&["25"], &[("25", true, true, None, None)], &[]).is_none());
}

/// 이름이 주어진 순서로 Java 목록을 만들고 정렬된 판 이름을 돌려준다(원본 `num` 으로 정렬)
fn java_order(names: &[&'static str]) -> Vec<String> {
    let mut g = feeds_today();
    g.insert(Feed::Spring, spring_meta(Some("4.1.2"), &["4.1.2"], names));
    let rows: Vec<Row> = names.iter().map(|n| (*n, true, false, None, None)).collect();
    g.insert(Feed::Temurin, eol(&rows, &[]));
    g.insert(Feed::SpringBoot, eol(SPRING_BOOT, &[]));
    built(&g).unwrap().list("java").iter().map(|v| v.v.clone()).collect()
}

#[test]
fn extra_판_이름_수로_정렬() {
    // num: 앞 두 마디를 수 하나로 읽는다 — 3.9 가 3.15 보다 큰 값이다(원본 그대로). 읽을 수 없으면 0
    assert_eq!(java_order(&["8u504", "3.15", "3.9"]), ["3.9", "3.15", "8u504"]);
    assert_eq!(java_order(&["x.y", "21", "25"]), ["25", "21", "x.y"]);
    assert_eq!(java_order(&["17", "21", "25"]), ["25", "21", "17"]);
    assert_eq!(java_order(&["25", "21", "17"]), ["25", "21", "17"]);
    assert_eq!(java_order(&["17.0.21", "17.5.1", "9"]), ["17.5.1", "17.0.21", "9"]);
    // 같은 수는 들어온 차례를 지킨다
    assert_eq!(java_order(&["25.0", "25"]), ["25.0", "25"]);
    assert_eq!(java_order(&["25", "25.0"]), ["25", "25.0"]);
}

#[test]
fn extra_마디와_달은_목록을_거쳐_본다() {
    // majorMinor — npm latest 와 PyPI 판으로
    let mm = |latest: &str| {
        let mut g = feeds_today();
        g.insert(Feed::React, format!(r#"{{"latest":"{latest}"}}"#).into_bytes());
        built(&g).unwrap().list("react")[0].v.clone()
    };
    assert_eq!(mm("4.1.2"), "4.1");
    assert_eq!(mm("25"), "25");
    assert_eq!(mm("17.0.21"), "17.0");
    assert_eq!(mm("12.0.0-alpha.7"), "12.0");
    assert_eq!(mm("..1..2.3"), "1.2");
    assert_eq!(mm("."), "");
    let mut g = feeds_today();
    g.insert(Feed::Fastapi, br#"{"info":{"version":"17.0.21"}}"#.to_vec());
    assert_eq!(built(&g).unwrap().list("fastapi")[0].v, "17.0");
    // month — eolFrom 글의 앞 7자
    let until = |from: Option<&str>| eol_list(&[EolRelease::new("1", false, false, from, None, None)], 3)[0].until.clone();
    assert_eq!(until(Some("2031-09-30")).as_deref(), Some("2031-09"));
    assert_eq!(until(Some("2031-09")).as_deref(), Some("2031-09"));
    assert_eq!(until(Some("2031")).as_deref(), Some("2031"));
    assert_eq!(until(Some("")).as_deref(), Some(""));
    assert_eq!(until(None), None);
}

#[test]
fn extra_목록_갈래와_한도() {
    // 항목마다 보이는 개수: kotlin·mysql 둘, node·python·postgresql 셋
    let lts4: &[Row] = &[
        ("4", true, false, None, None),
        ("3", true, false, None, None),
        ("2", true, false, None, None),
        ("1", true, false, None, None),
    ];
    let mut g = feeds_today();
    for f in [Feed::Kotlin, Feed::Mysql, Feed::Node, Feed::Python, Feed::Postgresql] {
        g.insert(f, eol(lts4, &[]));
    }
    let c = built(&g).unwrap();
    assert_eq!(show(c.list("kotlin")), "4 LTS 추천, 3 LTS");
    assert_eq!(show(c.list("mysql")), "4 LTS 추천, 3 LTS");
    assert_eq!(show(c.list("node")), "4 LTS 추천, 3 LTS, 2 LTS");
    assert_eq!(show(c.list("python")), "4 LTS 추천, 3 LTS, 2 LTS");
    assert_eq!(show(c.list("postgresql")), "4 LTS 추천, 3 LTS, 2 LTS");
    // 항목 키 전부와 받은 시각
    let c = build_catalog(&feeds_today(), 1.5).unwrap();
    assert_eq!(c.at, 1.5);
    let keys: Vec<&str> = c.items.keys().map(String::as_str).collect();
    assert_eq!(keys, ["fastapi", "java", "kotlin", "mysql", "nest", "next", "node", "postgresql", "python", "react", "spring-boot", "vite", "vue"]);
    assert_eq!(c.notes.len(), 1);
    // NestJS: legacy 가 같은 앞 두 마디거나 없으면 하나
    let nest = |tags: &str| {
        let mut g = feeds_today();
        g.insert(Feed::Nest, tags.as_bytes().to_vec());
        show(built(&g).unwrap().list("nest"))
    };
    assert_eq!(nest(r#"{"latest":"12.3.2","legacy":"12.3.9"}"#), "12.3 추천");
    assert_eq!(nest(r#"{"latest":"12.3.2"}"#), "12.3 추천");
    assert_eq!(nest(r#"{"latest":"12.3.2","legacy":"11.2.8"}"#), "12.3 추천, 11.2");
    // 받은 곳 하나라도 없으면, 또는 풀 수 없으면 만들지 않는다
    for f in Feed::ALL {
        let mut g = feeds_today();
        g.remove(&f);
        assert!(built(&g).is_none(), "{f:?} 없음");
        let mut g = feeds_today();
        g.insert(f, b"<html>".to_vec());
        assert!(built(&g).is_none(), "{f:?} 깨짐");
    }
    assert!(built(&HashMap::new()).is_none());
}

#[test]
fn extra_캐시_글_꼴() {
    let mut items = BTreeMap::new();
    items.insert("java".to_string(), vec![Ver::new("25", true, Some("2031-09"), true)]);
    let mut notes = BTreeMap::new();
    notes.insert("java".to_string(), "메모".to_string());
    let c = Catalog::new(1793286911.0, items.clone(), notes);
    // 원본 JSONEncoder(.sortedKeys) 와 같은 글자 — 키 정렬, 정수 시각은 소수점 없이, 한글은 그대로
    assert_eq!(
        String::from_utf8(catalog_data(&c)).unwrap(),
        r#"{"at":1793286911,"items":{"java":[{"lts":true,"rec":true,"until":"2031-09","v":"25"}]},"notes":{"java":"메모"}}"#
    );
    assert_eq!(catalog(&catalog_data(&c)), Some(c.clone()));
    let frac = Catalog::new(1.5, items, BTreeMap::new());
    assert_eq!(String::from_utf8(catalog_data(&frac)).unwrap(), r#"{"at":1.5,"items":{"java":[{"lts":true,"rec":true,"until":"2031-09","v":"25"}]},"notes":{}}"#);
    assert_eq!(catalog(&catalog_data(&frac)), Some(frac));
    // 쓸 수 없는 시각이면 빈 글
    assert!(catalog_data(&Catalog::new(f64::NAN, BTreeMap::new(), BTreeMap::new())).is_empty());
    assert!(catalog_data(&Catalog::new(f64::INFINITY, BTreeMap::new(), BTreeMap::new())).is_empty());
    // 앱에 든 목록은 다시 써도 글자가 같다
    let b = catalog(BUILTIN_CATALOG_JSON.as_bytes()).unwrap();
    assert_eq!(catalog_data(&b), BUILTIN_CATALOG_JSON.as_bytes());
    // 풀 수 없는 글
    for bad in ["", "<html>", "[]", "{}", r#"{"at":1,"items":{}}"#, r#"{"at":"x","items":{},"notes":{}}"#] {
        assert_eq!(catalog(bad.as_bytes()), None, "{bad}");
    }
    assert_eq!(catalog(br#"{"at":2,"items":{},"notes":{}}"#), Some(Catalog::new(2.0, BTreeMap::new(), BTreeMap::new())));
}

// ── 불러오기(하루 캐시·받기 실패·앱에 든 목록) ─────────────────────────────────

struct FakeFeeds {
    got: HashMap<Feed, Vec<u8>>,
    asked: Mutex<Vec<Feed>>,
}
impl FakeFeeds {
    fn new(got: HashMap<Feed, Vec<u8>>) -> FakeFeeds {
        FakeFeeds { got, asked: Mutex::new(Vec::new()) }
    }
    fn asked_any(&self) -> bool {
        !self.asked.lock().unwrap().is_empty()
    }
}
impl VersionFeeds for FakeFeeds {
    fn fetch(&self, feed: Feed) -> Option<Vec<u8>> {
        self.asked.lock().unwrap().push(feed);
        self.got.get(&feed).cloned()
    }
}

struct FakeCache {
    data: Mutex<Option<Vec<u8>>>,
    at: Option<f64>,
    wrote: Mutex<Vec<Vec<u8>>>,
}
impl FakeCache {
    fn new(data: Option<Vec<u8>>, at: Option<f64>) -> FakeCache {
        FakeCache { data: Mutex::new(data), at, wrote: Mutex::new(Vec::new()) }
    }
    fn wrote_any(&self) -> bool {
        !self.wrote.lock().unwrap().is_empty()
    }
}
impl VersionCache for FakeCache {
    fn read(&self) -> Option<(Vec<u8>, f64)> {
        let d = self.data.lock().unwrap().clone()?;
        Some((d, self.at?))
    }
    fn write(&self, data: &[u8]) {
        self.wrote.lock().unwrap().push(data.to_vec());
        *self.data.lock().unwrap() = Some(data.to_vec());
    }
}

struct FakeClock {
    now: f64,
}
impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.now
    }
    fn sleep(&self, _d: Duration) {}
}

struct LoadRow {
    note: &'static str,
    /// 초(`None` 이면 캐시 없음)
    cache_age: Option<f64>,
    /// 받기를 켰나
    feeds: bool,
    feeds_ok: bool,
    want: CatalogOrigin,
    /// 받기를 불렀나
    asks: bool,
    writes: bool,
}

const NOW: f64 = 1_793_300_000.0;

fn old_catalog() -> Catalog {
    let mut items = BTreeMap::new();
    items.insert("java".to_string(), vec![Ver::new("21", true, None, true)]);
    Catalog::new(1.0, items, BTreeMap::new())
}

fn load(feeds: Option<&FakeFeeds>, cache: &FakeCache) -> Option<CatalogLoad> {
    load_catalog(feeds.map(|f| f as &dyn VersionFeeds), cache, &FakeClock { now: NOW }, BUILTIN_CATALOG_JSON.as_bytes())
}

fn cache_aged(age: Option<f64>) -> FakeCache {
    FakeCache::new(age.map(|_| catalog_data(&old_catalog())), age.map(|a| NOW - a))
}

mod catalog_load_tests {
    use super::*;

    #[test]
    fn 표() {
        // 하루 캐시와 받기 실패 갈래
        for r in LOAD_ROWS {
            let cache = cache_aged(r.cache_age);
            let mut g = feeds_today();
            if !r.feeds_ok {
                g.remove(&Feed::Fastapi);
            }
            let feeds = FakeFeeds::new(g);
            let l = load(if r.feeds { Some(&feeds) } else { None }, &cache);
            let note = r.note;
            assert_eq!(l.as_ref().map(|l| l.origin), Some(r.want), "{note}");
            assert_eq!(feeds.asked_any(), r.asks, "{note}");
            assert_eq!(cache.wrote_any(), r.writes, "{note}");
            let l = l.unwrap();
            if r.want == CatalogOrigin::Fresh {
                assert!(l.catalog.at == NOW && l.catalog.recommended("java") == Some("25"), "{note}");
            }
            if r.want == CatalogOrigin::Stale {
                assert!(l.catalog == old_catalog() && !l.failed.is_empty(), "{note}");
            }
            if !r.feeds_ok && r.feeds {
                assert_eq!(l.failed, ["PyPI"], "{note}");
            }
        }
    }

    #[test]
    fn 머리() {
        // 머리 글
        let c = Catalog::new(1.0, BTreeMap::new(), BTreeMap::new());
        let l = |o| CatalogLoad::new(c.clone(), o, Vec::new());
        assert_eq!(catalog_label(&l(CatalogOrigin::Fresh), "오늘 09:12"), "버전 목록 오늘 09:12 받음 · 공식 목록");
        assert_eq!(catalog_label(&l(CatalogOrigin::Builtin), "10월 30일"), "오래된 목록(10월 30일) · 받지 못해 앱에 든 목록을 씁니다");
    }
}

#[test]
fn extra_머리_글_넷() {
    let c = Catalog::new(1.0, BTreeMap::new(), BTreeMap::new());
    let l = |o| CatalogLoad::new(c.clone(), o, vec!["PyPI".to_string()]);
    assert_eq!(catalog_label(&l(CatalogOrigin::Cached), "어제"), "버전 목록 어제 받음 · 공식 목록");
    assert_eq!(catalog_label(&l(CatalogOrigin::Stale), "10월 30일"), "오래된 목록(10월 30일) · 받지 못해 앞서 받은 목록을 씁니다");
    let raws: Vec<&str> = [CatalogOrigin::Fresh, CatalogOrigin::Cached, CatalogOrigin::Stale, CatalogOrigin::Builtin].iter().map(|o| o.raw_value()).collect();
    assert_eq!(raws, ["fresh", "cached", "stale", "builtin"]);
}

#[test]
fn extra_하루_경계() {
    // 하루(86400초)가 되면 믿지 않는다 · 방금 받은 것(0초)과 하루 직전은 믿는다 · 앞날이면 믿지 않는다
    for (age, want) in [
        (0.0, CatalogOrigin::Cached),
        (86399.0, CatalogOrigin::Cached),
        (86400.0, CatalogOrigin::Fresh),
        (-0.5, CatalogOrigin::Fresh),
    ] {
        let cache = cache_aged(Some(age));
        let feeds = FakeFeeds::new(feeds_today());
        let l = load(Some(&feeds), &cache).unwrap();
        assert_eq!(l.origin, want, "{age}");
        assert_eq!(feeds.asked_any(), want == CatalogOrigin::Fresh, "{age}");
        if want == CatalogOrigin::Cached {
            assert_eq!(l.catalog, old_catalog());
            assert!(l.failed.is_empty());
        }
    }
}

#[test]
fn extra_받아서_쓰기() {
    let cache = cache_aged(None);
    let feeds = FakeFeeds::new(feeds_today());
    let l = load(Some(&feeds), &cache).unwrap();
    assert_eq!(l.origin, CatalogOrigin::Fresh);
    assert!(l.failed.is_empty());
    // 공식 목록 열네 곳을 모두 부르고(선언 차례), 만든 목록을 글로 한 번 쓴다
    assert_eq!(*feeds.asked.lock().unwrap(), Feed::ALL);
    let wrote = cache.wrote.lock().unwrap();
    assert_eq!(wrote.len(), 1);
    assert_eq!(wrote[0], catalog_data(&l.catalog));
    assert_eq!(catalog(&wrote[0]), Some(l.catalog.clone()));
    assert_eq!(l.catalog, build_catalog(&feeds_today(), NOW).unwrap());
}

#[test]
fn extra_받지_못한_곳_이름() {
    let without = |fs: &[Feed], cached: bool| {
        let mut g = feeds_today();
        for f in fs {
            g.remove(f);
        }
        let cache = cache_aged(cached.then_some(90000.0));
        let feeds = FakeFeeds::new(g);
        let l = load(Some(&feeds), &cache).unwrap();
        assert!(!cache.wrote_any());
        assert_eq!(feeds.asked.lock().unwrap().len(), 14, "한 곳이 안 와도 모두 부른다");
        l
    };
    // 같은 이름은 한 번만, 받으려던 차례대로
    assert_eq!(without(&[Feed::Nest, Feed::React, Feed::Next, Feed::Vue, Feed::Vite], true).failed, ["npm"]);
    assert_eq!(without(&[Feed::Temurin, Feed::Kotlin], true).failed, ["endoflife.date"]);
    assert_eq!(without(&[Feed::Fastapi, Feed::Vue, Feed::Spring], true).failed, ["start.spring.io", "npm", "PyPI"]);
    let l = without(&[Feed::Spring], false);
    assert_eq!((l.origin, l.failed), (CatalogOrigin::Builtin, vec!["start.spring.io".to_string()]));
    assert_eq!(l.catalog, catalog(BUILTIN_CATALOG_JSON.as_bytes()).unwrap());
}

#[test]
fn extra_응답_꼴이_달라_풀지_못함() {
    let mut g = feeds_today();
    g.insert(Feed::Temurin, b"<html>".to_vec());
    for (cached, want) in [(true, CatalogOrigin::Stale), (false, CatalogOrigin::Builtin)] {
        let cache = cache_aged(cached.then_some(90000.0));
        let feeds = FakeFeeds::new(g.clone());
        let l = load(Some(&feeds), &cache).unwrap();
        assert_eq!(l.origin, want);
        assert_eq!(l.failed, ["응답 꼴이 달라 풀지 못함"]);
        assert!(feeds.asked_any() && !cache.wrote_any());
    }
}

#[test]
fn extra_받기_꺼짐과_망가진_캐시() {
    let cache = cache_aged(Some(90000.0));
    let l = load(None, &cache).unwrap();
    assert_eq!((l.origin, l.failed.as_slice()), (CatalogOrigin::Stale, ["받기 꺼짐".to_string()].as_slice()));
    let cache = cache_aged(None);
    let l = load(None, &cache).unwrap();
    assert_eq!((l.origin, l.failed.as_slice()), (CatalogOrigin::Builtin, ["받기 꺼짐".to_string()].as_slice()));
    // 풀 수 없는 캐시는 없는 것과 같다(하루 안이어도 받는다)
    let cache = FakeCache::new(Some(b"<html>".to_vec()), Some(NOW - 10.0));
    let feeds = FakeFeeds::new(feeds_today());
    assert_eq!(load(Some(&feeds), &cache).unwrap().origin, CatalogOrigin::Fresh);
    assert!(feeds.asked_any() && cache.wrote_any());
    // 앱에 든 목록도 풀 수 없으면 아무것도 없다
    let cache = cache_aged(None);
    assert_eq!(load_catalog(None, &cache, &FakeClock { now: NOW }, b"<html>"), None);
    assert_eq!(load_catalog(None, &cache, &FakeClock { now: NOW }, b""), None);
}

/// Swift `catalogRows`(변환 스크립트가 옮긴 것): (note, key, want)
const CATALOG_ROWS: [(&str, &str, &str); 13] = [
    ("Spring Boot: GA 만(SNAPSHOT·RC 뺌), 지원 중인 줄, 추천은 start.spring.io 기본값", "spring-boot", "4.1.2 추천, 4.0.9"),
    ("Java: 지원 중인 LTS ∩ start.spring.io ∩ Spring Boot 4.1 범위(17 - 26) — 27(LTS 아님)·11·8(start.spring.io 에 없음) 뺌, 기본값 17 대신 최신 LTS 25 추천", "java", "25 LTS 추천, 21 LTS, 17 LTS"),
    ("Node.js: LTS 줄이 있으면 LTS 만(27 은 LTS 아님, 20 은 지원 끝), 최신 LTS 추천", "node", "26 LTS 추천, 24 LTS, 22 LTS"),
    ("Python: LTS 개념이 없어 지원 중인 최신 셋, 최신 추천", "python", "3.15 추천, 3.14, 3.13"),
    ("PostgreSQL: 지원 중인 최신 셋", "postgresql", "18 추천, 17, 16"),
    ("MySQL: LTS 줄이 있으면 LTS 만(26.10 혁신판 뺌)", "mysql", "9.7 LTS 추천, 8.4 LTS"),
    ("Kotlin: 지원 끝 날이 없는 지원 중인 줄", "kotlin", "2.4 추천"),
    ("NestJS: npm latest 추천과 앞 큰 판(legacy)", "nest", "12.3 추천, 11.2"),
    ("React·Vite·Next.js·Vue·FastAPI 는 latest 하나", "react", "19.3 추천"),
    ("Vite", "vite", "8.3 추천"),
    ("Next.js", "next", "16.4 추천"),
    ("Vue", "vue", "3.5 추천"),
    ("FastAPI(PyPI)", "fastapi", "0.162 추천"),
];

/// Swift `loadRows`(변환 스크립트가 옮긴 것)
const LOAD_ROWS: [LoadRow; 8] = [
    LoadRow { note: "하루 안의 캐시면 받지 않는다", cache_age: Some(3600.0), feeds: true, feeds_ok: true, want: CatalogOrigin::Cached, asks: false, writes: false },
    LoadRow { note: "하루 지난 캐시면 새로 받아 캐시에 쓴다", cache_age: Some(90000.0), feeds: true, feeds_ok: true, want: CatalogOrigin::Fresh, asks: true, writes: true },
    LoadRow { note: "캐시가 없으면 받는다", cache_age: None, feeds: true, feeds_ok: true, want: CatalogOrigin::Fresh, asks: true, writes: true },
    LoadRow { note: "받지 못하면 하루 지난 캐시(오래된 목록)", cache_age: Some(90000.0), feeds: true, feeds_ok: false, want: CatalogOrigin::Stale, asks: true, writes: false },
    LoadRow { note: "받지 못했고 캐시도 없으면 앱에 든 목록", cache_age: None, feeds: true, feeds_ok: false, want: CatalogOrigin::Builtin, asks: true, writes: false },
    LoadRow { note: "받기를 끄면 하루 지난 캐시", cache_age: Some(90000.0), feeds: false, feeds_ok: true, want: CatalogOrigin::Stale, asks: false, writes: false },
    LoadRow { note: "받기를 끄고 캐시도 없으면 앱에 든 목록", cache_age: None, feeds: false, feeds_ok: true, want: CatalogOrigin::Builtin, asks: false, writes: false },
    LoadRow { note: "캐시 시각이 앞날이면(시계를 되돌림) 믿지 않고 받는다", cache_age: Some(-7200.0), feeds: true, feeds_ok: true, want: CatalogOrigin::Fresh, asks: true, writes: true },
];
