//! 기술 스택 칸의 버전 목록 규칙. 원본: `Sources/AutopilotRules/Versions.swift`(230줄) — 전부 옮겼다. 원본 순서대로 옮긴 선언:
//! `Feed`(APFeed — 받을 곳 닫힌 목록·주소·Accept·이름) · `Ver`(APVer) · `Catalog`(APCatalog) · `SpringMeta`(APSpringMeta) · `parse_spring`(apParseSpring) ·
//! `EolRelease`(APEolRelease) · `parse_eol`(apParseEol) · `parse_npm_tags`(apParseNpmTags) · `parse_pypi`(apParsePyPI) ·
//! `major_minor`(majorMinor) · `month`(month) · `num`(num) · `eol_list`(apEolList) · `in_range`(inRange) · `build_catalog`(apBuildCatalog) ·
//! `catalog_data`(apCatalogData) · `catalog`(apCatalog) · `CatalogOrigin`(APCatalogOrigin) · `CatalogLoad`(APCatalogLoad) · `load_catalog`(apLoadCatalog) · `catalog_label`(apCatalogLabel).
//! 남은 것: 없다. 원본이 `private` 인 `major_minor`·`month`·`num`·`in_range` 는 여기서도 모듈 밖에 열지 않는다.
//!
//! 바깥은 포트로만 부른다. 공식 목록 받기(`VersionFeeds` = 원본 `APVersionFeeds`)와 하루 캐시(`VersionCache` = `APVersionCache`)는 이 파일의 트레잇이고, 시각은 `crate::ports::Clock` 이다.
//! 앱에 든 목록(원본 `BuiltinVersions.swift` 의 `apBuiltinCatalogData`)은 이 파일에 없다 — `load_catalog` 가 `builtin: &[u8]` 로 받는다.
//! 캐시가 파일이어도 경로는 어댑터 몫이다: 규칙은 경로 글자를 만지지 않으니 `\` 와 `/` 의 차이는 `VersionCache` 구현이 정한다.
//!
//! Swift ↔ Rust 로 달라진 점:
//! - `Data` 는 `&[u8]`, `[String: Any]` 는 `serde_json::Value`다. 원본 `JSONSerialization.jsonObject(with:)` 는 조각(fragment) 없이 읽으므로 최상위가 객체가 아니면 `None` 이다. 여기도 같다.
//!   `serde_json` 은 UTF-8 BOM 으로 시작하는 글을 거절한다.
//! - 원본의 `as? [[String: Any]]` 는 배열 원소 하나라도 객체가 아니면 통째로 실패한다. 같은 뜻으로 옮겼다(`parse_spring` 은 빈 목록, `parse_eol` 은 `None`).
//! - `as? Bool` 은 JSON `true`/`false` 와, 값이 0 또는 1 인 수(Foundation 의 `NSNumber` → `Bool` 브리지가 받는 값)를 받는다. 다른 수·글자는 거짓(기본값)이다.
//! - `as? String` 은 JSON 글자만 받는다(수·`null`·배열은 `None`).
//! - 원본 `Dictionary` 는 순서가 없고 여기는 `BTreeMap`(키 정렬)이다. 원본이 `.sortedKeys` 로 쓰므로 글로 나가는 순서가 같다.
//! - Swift `String.prefix(7)`·`split(separator: ".")` 는 글자 묶음(Character) 단위이고 여기는 코드 점(`char`) 단위다. 날짜·버전 글은 ASCII 라 같다.
//! - `num` 은 `Double(String)`(Swift)이 아닌 `f64::from_str`(Rust)이다. 숫자·소수점 글자는 같고, 16진 부동소수(`0x1p3`)는 Swift 만 받는다. 판 이름(`25`·`3.15`·`8u504`)에서는 같다.
//! - `eol_list` 의 `limit` 기본값 3 은 Rust 에 기본 인자가 없어 호출하는 쪽이 3 을 준다. `limit` 이 음수면 Swift 는 죽고 여기 `usize` 는 음수가 없다.
//! - `Ver`·`Catalog` 는 `serde` 로 쓰고 읽는다(Swift `Codable`). `until` 이 없으면 글에서 키를 뺀다(Swift `encodeIfPresent`). 원본 합성 `Decodable` 은 기본값이 있는 `lts`·`rec`·`notes` 도
//!   키가 없으면 실패한다 — 여기도 `#[serde(default)]` 를 두지 않아 같다.
//! - 시각은 유닉스 초(`f64`)다. 원본 `Date` 의 `timeIntervalSince` 는 뺄셈이다. 캐시 글은 `catalog_data` 가 원본 `JSONEncoder(.sortedKeys)` 와 같은 글자로 쓴다(키 정렬 `lts`·`rec`·`until`·`v`, 정수인 시각은 소수점 없이).
//! - `in_range` 가 범위를 쪼갠 마디의 앞뒤 공백은 `trimmingCharacters(in: .whitespaces)` 와 같게(탭과 가로 공백 글자만) 깎는다. 마디 읽기의 `Double(String)` 과 `f64::from_str` 차이는 위 `num` 과 같다.
//! - `num` 이 `NaN` 이 되는 글(`nan`)이 판 이름이면 정렬 순서가 Swift 와 다를 수 있다(여기는 `total_cmp`). 그런 Temurin 줄은 없다.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

use crate::ports::Clock;

// ── 받을 곳 ─────────────────────────────────

/// 받을 곳 — 닫힌 목록이라 규칙·어댑터가 이 밖의 주소를 부를 수 없다(공개 읽기만). 원본: `APFeed`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feed {
    Spring,
    Temurin,
    SpringBoot,
    Node,
    Python,
    Postgresql,
    Mysql,
    Kotlin,
    Nest,
    React,
    Next,
    Vue,
    Vite,
    Fastapi,
}

impl Feed {
    /// 원본 `CaseIterable.allCases`(선언 순서)
    pub const ALL: [Feed; 14] = [
        Feed::Spring,
        Feed::Temurin,
        Feed::SpringBoot,
        Feed::Node,
        Feed::Python,
        Feed::Postgresql,
        Feed::Mysql,
        Feed::Kotlin,
        Feed::Nest,
        Feed::React,
        Feed::Next,
        Feed::Vue,
        Feed::Vite,
        Feed::Fastapi,
    ];

    /// 원본 `rawValue`(`String` 원시값 — 선언한 이름 그대로)
    pub fn raw_value(self) -> &'static str {
        match self {
            Feed::Spring => "spring",
            Feed::Temurin => "temurin",
            Feed::SpringBoot => "springBoot",
            Feed::Node => "node",
            Feed::Python => "python",
            Feed::Postgresql => "postgresql",
            Feed::Mysql => "mysql",
            Feed::Kotlin => "kotlin",
            Feed::Nest => "nest",
            Feed::React => "react",
            Feed::Next => "next",
            Feed::Vue => "vue",
            Feed::Vite => "vite",
            Feed::Fastapi => "fastapi",
        }
    }

    /// 받을 주소
    pub fn url(self) -> &'static str {
        match self {
            Feed::Spring => "https://start.spring.io/",
            Feed::Temurin => "https://endoflife.date/api/v1/products/eclipse-temurin",
            Feed::SpringBoot => "https://endoflife.date/api/v1/products/spring-boot",
            Feed::Node => "https://endoflife.date/api/v1/products/nodejs",
            Feed::Python => "https://endoflife.date/api/v1/products/python",
            Feed::Postgresql => "https://endoflife.date/api/v1/products/postgresql",
            Feed::Mysql => "https://endoflife.date/api/v1/products/mysql",
            Feed::Kotlin => "https://endoflife.date/api/v1/products/kotlin",
            Feed::Nest => "https://registry.npmjs.org/-/package/@nestjs/core/dist-tags",
            Feed::React => "https://registry.npmjs.org/-/package/react/dist-tags",
            Feed::Next => "https://registry.npmjs.org/-/package/next/dist-tags",
            Feed::Vue => "https://registry.npmjs.org/-/package/vue/dist-tags",
            Feed::Vite => "https://registry.npmjs.org/-/package/vite/dist-tags",
            Feed::Fastapi => "https://pypi.org/pypi/fastapi/json",
        }
    }

    /// start.spring.io 는 이 Accept 일 때 메타데이터(JSON)를 준다
    pub fn accept(self) -> &'static str {
        if self == Feed::Spring {
            "application/vnd.initializr.v2.3+json"
        } else {
            "application/json"
        }
    }

    /// 받지 못한 곳을 알리는 이름
    pub fn label(self) -> &'static str {
        match self {
            Feed::Spring => "start.spring.io",
            Feed::Nest | Feed::React | Feed::Next | Feed::Vue | Feed::Vite => "npm",
            Feed::Fastapi => "PyPI",
            _ => "endoflife.date",
        }
    }
}

// ── 목록 ─────────────────────────────────

/// 목록의 판 하나. 원본: `APVer`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ver {
    // 필드 순서는 원본 `JSONEncoder(.sortedKeys)` 의 글 순서(lts·rec·until·v)다
    pub lts: bool,
    /// 추천
    pub rec: bool,
    /// 지원 끝(yyyy-MM)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// 보이는 판(예: 25, 4.1.2, 19.3)
    pub v: String,
}

impl Ver {
    /// 원본 `init(v:lts:until:rec:)`(기본값 `false`·`nil`·`false` 는 호출하는 쪽이 준다)
    pub fn new(v: &str, lts: bool, until: Option<&str>, rec: bool) -> Ver {
        Ver { v: v.to_string(), lts, until: until.map(str::to_string), rec }
    }
}

/// 버전 목록 — 항목 키(java·spring-boot·kotlin·node·nest·python·fastapi·react·vite·next·vue·postgresql·mysql)마다 판 목록, 받은 시각, 항목 메모. 원본: `APCatalog`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// 받은 시각(유닉스 초). 글로 쓸 때 정수면 소수점 없이 쓴다(원본 `JSONEncoder`)
    #[serde(serialize_with = "ser_at")]
    pub at: f64,
    pub items: BTreeMap<String, Vec<Ver>>,
    pub notes: BTreeMap<String, String>,
}

fn ser_at<S: serde::Serializer>(at: &f64, s: S) -> Result<S::Ok, S::Error> {
    if at.fract() == 0.0 && at.abs() < 9.0e15 {
        s.serialize_i64(*at as i64)
    } else {
        s.serialize_f64(*at)
    }
}

impl Catalog {
    pub fn new(at: f64, items: BTreeMap<String, Vec<Ver>>, notes: BTreeMap<String, String>) -> Catalog {
        Catalog { at, items, notes }
    }

    /// 항목의 판 목록(없으면 빈 목록)
    pub fn list(&self, key: &str) -> &[Ver] {
        self.items.get(key).map_or(&[], Vec::as_slice)
    }

    /// 추천 판: 추천 표시가 있는 첫 판, 없으면 맨 앞 판, 항목이 없거나 비면 `None`
    pub fn recommended(&self, key: &str) -> Option<&str> {
        let list = self.items.get(key)?;
        list.iter().find(|x| x.rec).or_else(|| list.first()).map(|x| x.v.as_str())
    }
}

// ── 응답 풀기 ─────────────────────────────────

/// start.spring.io 메타데이터에서 판정에 쓰는 칸. 원본: `APSpringMeta`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpringMeta {
    /// GA 만(-SNAPSHOT·-RC·-M 은 뺌), 응답 차례(최신부터)
    pub boots: Vec<String>,
    pub boot_default: Option<String>,
    pub javas: Vec<String>,
}

/// `as? String`
fn as_text(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

/// `as? Bool` — JSON 불리언, 또는 0·1 인 수
fn as_bool(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_f64() {
            Some(0.0) => Some(false),
            Some(1.0) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// `as? [[String: Any]]` — 원소 하나라도 객체가 아니면 `None`
fn as_objects(v: Option<&Value>) -> Option<Vec<&serde_json::Map<String, Value>>> {
    v.and_then(Value::as_array)?.iter().map(Value::as_object).collect()
}

/// 최상위가 객체인 JSON 만 읽는다(`JSONSerialization` 의 조각 없음)
fn parse_object(d: &[u8]) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_slice::<Value>(d).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// start.spring.io 응답 풀기. `bootVersion`·`javaVersion` 이 객체가 아니거나 GA 판·Java 판이 하나도 없으면 `None`. 원본: `apParseSpring`
pub fn parse_spring(d: &[u8]) -> Option<SpringMeta> {
    let j = parse_object(d)?;
    let b = j.get("bootVersion")?.as_object()?;
    let jv = j.get("javaVersion")?.as_object()?;
    let ids = |o: &serde_json::Map<String, Value>| -> Vec<String> {
        as_objects(o.get("values")).unwrap_or_default().iter().filter_map(|x| as_text(x.get("id"))).collect()
    };
    let m = SpringMeta {
        boots: ids(b).into_iter().filter(|s| !s.contains('-') && !s.contains("SNAPSHOT")).collect(),
        boot_default: as_text(b.get("default")).filter(|s| !s.contains('-')),
        javas: ids(jv),
    };
    if m.boots.is_empty() || m.javas.is_empty() {
        None
    } else {
        Some(m)
    }
}

/// endoflife.date 의 판 줄 하나. 원본: `APEolRelease`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EolRelease {
    pub name: String,
    pub lts: bool,
    pub eol: bool,
    pub eol_from: Option<String>,
    pub latest: Option<String>,
    /// spring-boot 의 custom.supportedJavaVersions("17 - 26")
    pub supported_java: Option<String>,
}

impl EolRelease {
    /// 원본 `init(name:lts:eol:eolFrom:latest:supportedJava:)`(기본값은 호출하는 쪽이 준다)
    pub fn new(name: &str, lts: bool, eol: bool, eol_from: Option<&str>, latest: Option<&str>, supported_java: Option<&str>) -> EolRelease {
        EolRelease {
            name: name.to_string(),
            lts,
            eol,
            eol_from: eol_from.map(str::to_string),
            latest: latest.map(str::to_string),
            supported_java: supported_java.map(str::to_string),
        }
    }
}

/// endoflife.date 응답 풀기. `result.releases` 가 객체 배열이 아니거나 `name` 이 글자인 줄이 하나도 없으면 `None`. 원본: `apParseEol`
pub fn parse_eol(d: &[u8]) -> Option<Vec<EolRelease>> {
    let j = parse_object(d)?;
    let rel = as_objects(j.get("result")?.as_object()?.get("releases"))?;
    let out: Vec<EolRelease> = rel
        .iter()
        .filter_map(|r| {
            let name = as_text(r.get("name"))?;
            Some(EolRelease {
                name,
                lts: as_bool(r.get("isLts")).unwrap_or(false),
                eol: as_bool(r.get("isEol")).unwrap_or(false),
                eol_from: as_text(r.get("eolFrom")),
                latest: as_text(r.get("latest").and_then(Value::as_object).and_then(|o| o.get("name"))),
                supported_java: as_text(r.get("custom").and_then(Value::as_object).and_then(|o| o.get("supportedJavaVersions"))),
            })
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// npm dist-tags 응답 풀기. `latest` 가 빈 글이 아닌 글자여야 하고, 값이 글자인 태그만 남긴다. 원본: `apParseNpmTags`
pub fn parse_npm_tags(d: &[u8]) -> Option<BTreeMap<String, String>> {
    let j = parse_object(d)?;
    match j.get("latest").and_then(Value::as_str) {
        Some(l) if !l.is_empty() => {}
        _ => return None,
    }
    Some(j.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
}

/// PyPI 응답에서 `info.version`. 원본: `apParsePyPI`
pub fn parse_pypi(d: &[u8]) -> Option<String> {
    let j = parse_object(d)?;
    as_text(j.get("info").and_then(Value::as_object).and_then(|o| o.get("version")))
}

// ── 목록 만들기 ─────────────────────────────────

/// 앞 두 마디만("4.1.2" → "4.1", "25" → "25"). 빈 마디는 건너뛴다. 원본: `majorMinor`
fn major_minor(v: &str) -> String {
    v.split('.').filter(|p| !p.is_empty()).take(2).collect::<Vec<_>>().join(".")
}

/// 날짜 글의 앞 7자(yyyy-MM). 원본: `month`
fn month(d: Option<&str>) -> Option<String> {
    d.map(|s| s.chars().take(7).collect())
}

/// 판 이름을 앞 두 마디 수로(읽을 수 없으면 0). 원본: `num`
fn num(s: &str) -> f64 {
    major_minor(s).parse::<f64>().unwrap_or(0.0)
}

/// endoflife 목록에서 보일 판 — 지원 중(isEol 아님)인 것, LTS 줄이 있으면 LTS 만(설계: 최신 LTS 추천), 최신부터 `limit` 개. 맨 앞이 추천. 원본: `apEolList`(기본 `limit` 3)
pub fn eol_list(rel: &[EolRelease], limit: usize) -> Vec<Ver> {
    let live: Vec<&EolRelease> = rel.iter().filter(|r| !r.eol).collect();
    let keep: Vec<&EolRelease> = if live.iter().any(|r| r.lts) { live.into_iter().filter(|r| r.lts).collect() } else { live };
    keep.into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, r)| Ver { v: r.name.clone(), lts: r.lts, until: month(r.eol_from.as_deref()), rec: i == 0 })
        .collect()
}

/// Spring Boot 가 받치는 Java 범위("17 - 26") 안에 드는가. 범위가 없거나 마디가 둘이 아니거나 `java` 가 수가 아니면 안에 든 것으로 본다. 원본: `inRange`
fn in_range(java: &str, range: Option<&str>) -> bool {
    let Some(r) = range else { return true };
    // 원본 `split(separator:)` 는 빈 조각을 버린다
    let p: Vec<f64> = r.split('-').filter(|x| !x.is_empty()).map(|x| x.trim_matches(is_h_space).parse::<f64>().unwrap_or(0.0)).collect();
    let (2, Ok(j)) = (p.len(), java.parse::<f64>()) else { return true };
    j >= p[0] && j <= p[1]
}

/// `CharacterSet.whitespaces` — 탭과 가로 공백 글자(줄바꿈은 아니다)
fn is_h_space(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

/// 받은 응답들로 목록을 만든다. 하나라도 없거나 풀 수 없으면 `None`(앞 목록으로 간다). 원본: `apBuildCatalog`
pub fn build_catalog(got: &HashMap<Feed, Vec<u8>>, at: f64) -> Option<Catalog> {
    let spring = parse_spring(got.get(&Feed::Spring)?)?;
    let eol_of = |f: Feed| got.get(&f).and_then(|d| parse_eol(d));
    let tags_of = |f: Feed| got.get(&f).and_then(|d| parse_npm_tags(d));
    let tem = eol_of(Feed::Temurin)?;
    let sb = eol_of(Feed::SpringBoot)?;
    let node = eol_of(Feed::Node)?;
    let py = eol_of(Feed::Python)?;
    let pg = eol_of(Feed::Postgresql)?;
    let my = eol_of(Feed::Mysql)?;
    let kt = eol_of(Feed::Kotlin)?;
    let nest = tags_of(Feed::Nest)?;
    let react = tags_of(Feed::React)?;
    let next = tags_of(Feed::Next)?;
    let vue = tags_of(Feed::Vue)?;
    let vite = tags_of(Feed::Vite)?;
    let fast = got.get(&Feed::Fastapi).and_then(|d| parse_pypi(d))?;
    let mut items: BTreeMap<String, Vec<Ver>> = BTreeMap::new();
    let mut notes: BTreeMap<String, String> = BTreeMap::new();
    // Spring Boot: GA 판 가운데 지원 중인 줄(endoflife 의 4.1·4.0 …)만. 추천은 start.spring.io 기본값(GA 일 때), 아니면 맨 앞
    let mut boots: Vec<Ver> = Vec::new();
    for b in &spring.boots {
        let Some(r) = sb.iter().find(|r| r.name == major_minor(b)) else { continue };
        if r.eol {
            continue;
        }
        if !boots.iter().any(|x| major_minor(&x.v) == major_minor(b)) {
            boots.push(Ver::new(b, false, month(r.eol_from.as_deref()).as_deref(), false));
        }
    }
    if boots.is_empty() {
        return None;
    }
    let rec_boot = boots.iter().position(|x| Some(&x.v) == spring.boot_default.as_ref()).unwrap_or(0);
    boots[rec_boot].rec = true;
    let rec_boot_v = boots[rec_boot].v.clone();
    items.insert("spring-boot".to_string(), boots.iter().take(3).cloned().collect());
    // Java: start.spring.io 가 고르게 하는 판 ∩ Temurin 의 지원 중인 LTS ∩ 추천 Spring Boot 가 받치는 범위. 추천은 그 가운데 최신
    let range: Option<&str> = sb.iter().find(|r| r.name == major_minor(&rec_boot_v)).and_then(|r| r.supported_java.as_deref());
    let lts: Vec<&EolRelease> = tem.iter().filter(|r| r.lts && !r.eol).collect();
    let mut java: Vec<Ver> = Vec::new();
    for j in &spring.javas {
        let Some(t) = lts.iter().find(|r| &r.name == j) else { continue };
        if !in_range(j, range) {
            continue;
        }
        java.push(Ver::new(j, true, month(t.eol_from.as_deref()).as_deref(), false));
    }
    java.sort_by(|a, b| num(&b.v).total_cmp(&num(&a.v)));
    if java.is_empty() {
        return None;
    }
    java[0].rec = true;
    items.insert("java".to_string(), java.iter().take(3).cloned().collect());
    let left: Vec<&str> = spring.javas.iter().filter(|j| !java.iter().any(|x| &x.v == *j)).map(String::as_str).collect();
    if !left.is_empty() {
        notes.insert(
            "java".to_string(),
            format!("목록에 없는 판: Java {}(LTS 가 아니거나 Spring Boot {} 의 범위 {} 밖)", left.join("·"), major_minor(&rec_boot_v), range.unwrap_or("모름")),
        );
    }
    items.insert("kotlin".to_string(), eol_list(&kt, 2));
    items.insert("node".to_string(), eol_list(&node, 3));
    items.insert("python".to_string(), eol_list(&py, 3));
    items.insert("postgresql".to_string(), eol_list(&pg, 3));
    items.insert("mysql".to_string(), eol_list(&my, 2));
    // npm: latest 를 추천으로, NestJS 는 앞 큰 판(legacy)도
    let latest = |t: &BTreeMap<String, String>| major_minor(t.get("latest").map_or("", String::as_str));
    let mut n = vec![Ver::new(&latest(&nest), false, None, true)];
    if let Some(l) = nest.get("legacy") {
        if major_minor(l) != n[0].v {
            n.push(Ver::new(&major_minor(l), false, None, false));
        }
    }
    items.insert("nest".to_string(), n);
    for (k, t) in [("react", &react), ("next", &next), ("vue", &vue), ("vite", &vite)] {
        items.insert(k.to_string(), vec![Ver::new(&latest(t), false, None, true)]);
    }
    items.insert("fastapi".to_string(), vec![Ver::new(&major_minor(&fast), false, None, true)]);
    Some(Catalog { at, items, notes })
}

/// 목록을 캐시 글로(키 정렬). 쓸 수 없으면 빈 글. 원본: `apCatalogData`
pub fn catalog_data(c: &Catalog) -> Vec<u8> {
    if !c.at.is_finite() {
        return Vec::new();
    }
    serde_json::to_vec(c).unwrap_or_default()
}

/// 캐시 글에서 목록을. 원본: `apCatalog`
pub fn catalog(d: &[u8]) -> Option<Catalog> {
    serde_json::from_slice(d).ok()
}

// ── 불러오기 ─────────────────────────────────

/// 어디서 온 목록인가. 원본: `APCatalogOrigin`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogOrigin {
    /// 지금 공식 목록을 받음
    Fresh,
    /// 하루 안에 받은 캐시
    Cached,
    /// 받지 못해 하루 지난 캐시(오래된 목록)
    Stale,
    /// 받지 못했고 캐시도 없어 앱에 든 목록(오래된 목록)
    Builtin,
}

impl CatalogOrigin {
    /// 원본 `rawValue`
    pub fn raw_value(self) -> &'static str {
        match self {
            CatalogOrigin::Fresh => "fresh",
            CatalogOrigin::Cached => "cached",
            CatalogOrigin::Stale => "stale",
            CatalogOrigin::Builtin => "builtin",
        }
    }
}

/// 불러온 목록과 출처. 원본: `APCatalogLoad`
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogLoad {
    pub catalog: Catalog,
    pub origin: CatalogOrigin,
    /// 받지 못한 곳(주소 이름)
    pub failed: Vec<String>,
}

impl CatalogLoad {
    /// 원본 `init(catalog:origin:failed:)`(기본 `failed` 는 빈 목록 — 호출하는 쪽이 준다)
    pub fn new(catalog: Catalog, origin: CatalogOrigin, failed: Vec<String>) -> CatalogLoad {
        CatalogLoad { catalog, origin, failed }
    }
}

/// 공식 목록 받기 — 고정한 공개 주소(`Feed`)만 인증 없이 GET 한다. 사용자 자료는 보내지 않는다. 200 일 때만 본문. 원본: `APVersionFeeds`
pub trait VersionFeeds: Send + Sync {
    fn fetch(&self, feed: Feed) -> Option<Vec<u8>>;
}

/// 받은 목록의 하루 캐시 — 내용과 쓴 시각(유닉스 초). 원본: `APVersionCache`
pub trait VersionCache: Send + Sync {
    fn read(&self) -> Option<(Vec<u8>, f64)>;
    fn write(&self, data: &[u8]);
}

/// 유스케이스: 버전 목록 불러오기 — 하루 안의 캐시면 그것, 아니면 공식 목록을 모두 받아 만들고 캐시에 쓴다. 받거나 만들지 못하면
/// 하루 지난 캐시(오래된 목록), 그것도 없으면 앱에 든 목록. `feeds` 가 `None` 이면 받지 않는다(받기를 끈 시험 빌드 등). 원본: `apLoadCatalog`
pub fn load_catalog(feeds: Option<&dyn VersionFeeds>, cache: &dyn VersionCache, clock: &dyn Clock, builtin: &[u8]) -> Option<CatalogLoad> {
    let now = clock.now();
    let cached: Option<(Catalog, f64)> = cache.read().and_then(|(data, at)| catalog(&data).map(|c| (c, at)));
    if let Some((c, at)) = &cached {
        if now - at < 86400.0 && now >= *at {
            return Some(CatalogLoad::new(c.clone(), CatalogOrigin::Cached, Vec::new()));
        }
    }
    let mut failed: Vec<String> = Vec::new();
    if let Some(feeds) = feeds {
        let mut got: HashMap<Feed, Vec<u8>> = HashMap::new();
        for f in Feed::ALL {
            if let Some(d) = feeds.fetch(f) {
                got.insert(f, d);
            } else if !failed.iter().any(|x| x == f.label()) {
                failed.push(f.label().to_string());
            }
        }
        if failed.is_empty() {
            if let Some(c) = build_catalog(&got, now) {
                cache.write(&catalog_data(&c));
                return Some(CatalogLoad::new(c, CatalogOrigin::Fresh, Vec::new()));
            }
            failed.push("응답 꼴이 달라 풀지 못함".to_string());
        }
    } else {
        failed.push("받기 꺼짐".to_string());
    }
    if let Some((c, _)) = cached {
        return Some(CatalogLoad::new(c, CatalogOrigin::Stale, failed));
    }
    catalog(builtin).map(|c| CatalogLoad::new(c, CatalogOrigin::Builtin, failed))
}

/// 목록 머리 글 — "버전 목록 오늘 09:12 받음 · 공식 목록" / "오래된 목록(10월 30일) · 앱에 든 목록"(날짜 글자는 보드가 만든다). 원본: `apCatalogLabel`
pub fn catalog_label(l: &CatalogLoad, date: &str) -> String {
    match l.origin {
        CatalogOrigin::Fresh | CatalogOrigin::Cached => format!("버전 목록 {date} 받음 · 공식 목록"),
        CatalogOrigin::Stale => format!("오래된 목록({date}) · 받지 못해 앞서 받은 목록을 씁니다"),
        CatalogOrigin::Builtin => format!("오래된 목록({date}) · 받지 못해 앱에 든 목록을 씁니다"),
    }
}
