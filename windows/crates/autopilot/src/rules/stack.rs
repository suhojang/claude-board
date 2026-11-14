//! 주제로 시작의 기술 스택 칸 셋(프론트·백엔드·DB)·기존 저장소 감지·주제 글 낱말 대조 규칙. 원본: `Sources/AutopilotRules/Stack.swift`(전체).
//! 순수 함수만 있다(파일·시각·환경 없음). 읽은 파일(상대 경로 → 글)과 주제 글에서 칸 셋을 정한다. 읽는 경로는 뿌리 기준 상대 경로이고 구분자는 `/` 다
//! (Windows 호출자는 `/` 로 바꾼 경로를 넘긴다. `StackFiles::read` 가 뿌리에 이어 붙이는 일은 어댑터 몫이다).
//!
//! 이식된 선언(원본 순서): `StackSlot`(APStackSlot) · `StackSet`(APStackSet, `is_empty`) · `STACK_DIRS`(apStackDirs) · `STACK_FILE_NAMES`(apStackFileNames) ·
//! `stack_candidates`(apStackCandidates) · 비공개 `first`·`clean`·`npm_deps` · `detect_stack`(apDetectStack(_ files:)) ·
//! `StackFiles`(원본 `APStackFiles`, `Ports.swift`) · `detect_stack_in_root`(apDetectStack(root:files:)) · `TopicHit`(APTopicHit) · 비공개 `TOPIC_WORDS`(apTopicWords)·
//! `is_latin`(isLatin) · `topic_hits`(apTopicHits) · `topic_stack`(apTopicStack) · `stack_conflicts`(apStackConflicts).
//!
//! Foundation 과 다른 점:
//! - 원본은 `NSRegularExpression`(ICU) 이다. 이 크레이트에는 정규식 크레이트가 없어(Cargo.toml 은 통합자 소유) 모든 패턴을 바이트 스캐너로 손으로 옮겼다.
//!   패턴마다 원본 정규식을 주석으로 붙였고, 탐욕·되돌림이 결과를 바꾸는 자리는 같은 결과가 나오게 풀어 썼다(`requires-python` 은 아래).
//! - 대소문자 무시(`first` 의 `.caseInsensitive`)는 ASCII 만 접는다. ICU 는 `ſ`(→s)·켈빈 기호(→k) 같은 비ASCII 도 접는다.
//! - `\s` 는 ICU 와 같게 `[\t\n\f\r\p{Z}]` 로 센다(Rust `char::is_whitespace` 가 아님: `\v`·U+0085 는 뺀다).
//! - `requires-python\s*=\s*"[^0-9]*([0-9.]+)` 는 첫 숫자가 없을 때 ICU 가 되돌려 `.` 하나를 잡을 수 있다. 여기는 None 이다.
//!   값은 `clean` 만 거치고 `.` 도 None 도 빈 글이 되므로 겉으로 같다.
//! - `String.contains`·`range(of:)` 는 원본이 정준 동치(결합 글자 순서) 기준이고 여기는 바이트 열 그대로다.
//! - `package.json` 읽기: 원본 `JSONSerialization`, 여기 `serde_json`(엄격한 JSON). BOM·NaN·아주 깊은 중첩 같은 Foundation 특유 입력은 다를 수 있다.
//!   의존성 판이 글이 아닌 값이면 원본은 `"\(v)"` 로 글로 바꾼다. 수는 같은 꼴(정수 값 실수는 정수로), 참/거짓은 `1`/`0`, null 은 `<null>`,
//!   배열·객체는 JSON 글이다(원본은 Swift 설명 글). 어느 쪽이든 `clean` 을 지나면 숫자로 시작하지 않아 빈 글이다.
//! - `clean` 의 앞 글자 깎기는 원본이 자소 묶음(`Character`) 단위, 여기는 `char` 단위다. 깎은 뒤 바로 숫자가 와야 하는 규칙이라 결과는 같다.
//! - 낱말 대조(`topic_hits`): 원본 `range(of:options:.caseInsensitive)` 는 정준 동치·ICU 접기 기준이고 여기는 ASCII 만 접는 바이트 대조다.
//!   낱말 표는 영문 ASCII 와 한글 완성형뿐이라 갈리는 곳은 결합 글자로 풀어 쓴 입력(`Java` + U+0301 처럼 낱말 끝이 글자 묶음 중간)뿐이다.
//! - 앞뒤 글자 검사의 `Character`(자소 묶음)는 `char` 로 센다. 뒤 글자가 ASCII 글자여도 그 뒤에 결합 글자가 붙으면 묶음이 ASCII 가 아니어서
//!   영문 글자로 보지 않는다(U+0300~036F·1AB0~1AFF·1DC0~1DFF·20D0~20FF·FE00~FE0F·FE20~FE2F·ZWNJ·ZWJ·피부색 U+1F3FB~1F3FF·U+E0100~E01EF). 그 밖 확장 글자는 모른다.
//! - 버전 꼴을 읽는 앞쪽 16 `Character` 는 `char` 16 개로 센다(읽는 글이 ASCII 뿐이고 결합 글자에서 끝나 같은 결과). 정규식 `^\s?v?([0-9]+(\.[0-9]+){0,2})(?![0-9])` 는
//!   바이트 스캐너로 옮겼다(탐욕 끝에서 뒤이은 글자는 숫자일 수 없어 `(?![0-9])` 는 항상 참이다).
//! - 글에 나온 차례 정렬은 원본 `sorted` 가 안정 정렬이라 여기도 안정 정렬이다(같은 낱말이 두 번이면 찾은 차례 그대로).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 칸 하나 — 이름(목록 이름 또는 감지한 이름), 언어·판, 프레임워크·판, 빌드 도구, 어디서 왔나(감지됨·주제에서 읽음·고름·직접 입력). 원본: `APStackSlot`
/// (Codable 과 같게 JSON 키는 원본 속성 이름 그대로 camelCase 다. 읽을 때 `sources` 까지 모든 키가 있어야 한다 — Swift 합성 `Decodable` 과 같다)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSlot {
    /// 목록 항목 키(spring-java·spring-kotlin·nest·fastapi·react·next·vue·postgresql·mysql), 직접 입력·목록 밖은 ""
    pub key: String,
    /// 사람에게 보이는 이름(예: "Java · Spring Boot")
    pub name: String,
    pub lang: String,
    pub lang_version: String,
    pub framework: String,
    pub framework_version: String,
    /// Gradle Groovy·Gradle Kotlin·Maven·npm 등
    pub build: String,
    /// detected·topic·picked·custom
    pub origin: String,
    /// 감지한 파일(상대 경로)
    pub sources: Vec<String>,
}

/// 칸 셋. 원본: `APStackSet`(비어 있는 칸은 JSON 에서 키를 뺀다 — Swift 합성 `Encodable` 의 `encodeIfPresent`)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StackSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<StackSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<StackSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<StackSlot>,
}

impl StackSet {
    pub fn is_empty(&self) -> bool {
        self.front.is_none() && self.back.is_none() && self.db.is_none()
    }
}

// MARK: - 기존 저장소 감지

/// 읽을 파일(저장소 뿌리와 흔한 하위 폴더). 이 밖은 읽지 않는다. 원본: `apStackDirs`
pub const STACK_DIRS: [&str; 8] = ["", "backend/", "frontend/", "server/", "client/", "web/", "api/", "app/"];
/// 원본: `apStackFileNames`
pub const STACK_FILE_NAMES: [&str; 15] = [
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "src/main/resources/application.yml",
    "src/main/resources/application.yaml",
    "src/main/resources/application.properties",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    "prisma/schema.prisma",
];

/// 읽을 상대 경로 전체(폴더 먼저, 그 안에서 파일 이름 차례). 원본: `apStackCandidates`
pub fn stack_candidates() -> Vec<String> {
    STACK_DIRS.iter().flat_map(|d| STACK_FILE_NAMES.iter().map(move |n| format!("{d}{n}"))).collect()
}

// ---------- 정규식 대신 쓰는 바이트 스캐너(위 머리 주석 참고) ----------

// ICU `\s` = [\t\n\f\r\p{Z}]
fn is_icu_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

// Foundation `CharacterSet.whitespaces`: 일반 범주 Zs 와 탭
fn is_cs_whitespace(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

// `\s*` — 다음 자리를 돌려준다
fn skip_space(s: &str, pos: usize) -> usize {
    let mut p = pos;
    for c in s[pos..].chars() {
        if is_icu_space(c) {
            p += c.len_utf8();
        } else {
            break;
        }
    }
    p
}

// 글자(ASCII) 그대로 또는 ASCII 대소문자 무시로 pos 에 literal 이 있는가
fn lit_at(s: &str, pos: usize, lit: &str, ci: bool) -> bool {
    let (b, l) = (s.as_bytes(), lit.as_bytes());
    let Some(end) = pos.checked_add(l.len()) else { return false };
    if end > b.len() {
        return false;
    }
    if ci {
        b[pos..end].eq_ignore_ascii_case(l)
    } else {
        &b[pos..end] == l
    }
}

// literal 이 나오는 모든 자리(왼쪽부터)
fn occurrences<'a>(s: &'a str, lit: &'a str, ci: bool) -> impl Iterator<Item = usize> + 'a {
    (0..s.len()).filter(move |&i| lit_at(s, i, lit, ci))
}

fn byte_at(s: &str, pos: usize) -> Option<u8> {
    s.as_bytes().get(pos).copied()
}

fn is_quote(s: &str, pos: usize) -> bool {
    matches!(byte_at(s, pos), Some(b'\'' | b'"'))
}

// pos 부터 ASCII 글자 pred 가 이어지는 바이트 수(비ASCII 바이트는 pred 가 거절해 글자 경계에서 멈춘다)
fn run_while(s: &str, pos: usize, pred: impl Fn(u8) -> bool) -> usize {
    s.as_bytes()[pos..].iter().take_while(|&&b| pred(b)).count()
}

fn is_digit_dot(b: u8) -> bool {
    b.is_ascii_digit() || b == b'.'
}

// `<open>([^<]+)<close>` 의 괄호 안(st 는 open 바로 뒤). [^<]+ 는 첫 `<` 까지가 전부다(더 줄여도 다음 글자가 `<` 일 수 없다)
fn text_before_lt(s: &str, st: usize, close: &str) -> Option<String> {
    let e = st + s[st..].find('<')?;
    if e > st && lit_at(s, e, close, true) {
        Some(s[st..e].to_string())
    } else {
        None
    }
}

// `<open>([^<]+)<close>` 가 나오는 가장 왼쪽 것
fn tagged(s: &str, open: &str, close: &str) -> Option<String> {
    occurrences(s, open, true).find_map(|p| text_before_lt(s, p + open.len(), close))
}

// `name\(\s*([0-9]+)\s*\)`
fn call_number(s: &str, name: &str) -> Option<String> {
    occurrences(s, name, true).find_map(|p| {
        let q = p + name.len();
        if byte_at(s, q) != Some(b'(') {
            return None;
        }
        let w = skip_space(s, q + 1);
        let e = w + run_while(s, w, |b| b.is_ascii_digit());
        if e == w {
            return None;
        }
        (byte_at(s, skip_space(s, e)) == Some(b')')).then(|| s[w..e].to_string())
    })
}

// `org\.springframework\.boot['"]?\)?\s+version\s+['"]([0-9][^'"]*)['"]`
fn spring_boot_version(g: &str) -> Option<String> {
    let lit = "org.springframework.boot";
    occurrences(g, lit, true).find_map(|p| {
        let mut q = p + lit.len();
        if is_quote(g, q) {
            q += 1;
        }
        if byte_at(g, q) == Some(b')') {
            q += 1;
        }
        let w = skip_space(g, q);
        if w == q || !lit_at(g, w, "version", true) {
            return None;
        }
        let q = w + "version".len();
        let w = skip_space(g, q);
        if w == q || !is_quote(g, w) {
            return None;
        }
        let st = w + 1;
        if !byte_at(g, st).is_some_and(|b| b.is_ascii_digit()) {
            return None;
        }
        let e = g[st..].find(['\'', '"'])?;
        Some(g[st..st + e].to_string())
    })
}

// `JavaVersion\.VERSION_([0-9_]+)`
fn java_version_underscore(g: &str) -> Option<String> {
    let lit = "JavaVersion.VERSION_";
    occurrences(g, lit, true).find_map(|p| {
        let st = p + lit.len();
        let n = run_while(g, st, |b| b.is_ascii_digit() || b == b'_');
        (n > 0).then(|| g[st..st + n].to_string())
    })
}

// `sourceCompatibility\s*=\s*['"]?([0-9.]+)`
fn source_compatibility(g: &str) -> Option<String> {
    let lit = "sourceCompatibility";
    occurrences(g, lit, true).find_map(|p| {
        let w = skip_space(g, p + lit.len());
        if byte_at(g, w) != Some(b'=') {
            return None;
        }
        let mut q = skip_space(g, w + 1);
        if is_quote(g, q) {
            q += 1;
        }
        let n = run_while(g, q, is_digit_dot);
        (n > 0).then(|| g[q..q + n].to_string())
    })
}

// `fastapi(\[[^\]]*\])?\s*(==|>=|~=|\^)?\s*"?([0-9][0-9.]*)` 의 셋째 괄호
fn fastapi_version(py: &str) -> Option<String> {
    let lit = "fastapi";
    occurrences(py, lit, true).find_map(|p| {
        let mut q = p + lit.len();
        if byte_at(py, q) == Some(b'[') {
            if let Some(e) = py[q + 1..].find(']') {
                q = q + 1 + e + 1;
            }
        }
        q = skip_space(py, q);
        if let Some(op) = ["==", ">=", "~=", "^"].into_iter().find(|op| lit_at(py, q, op, false)) {
            q += op.len();
        }
        q = skip_space(py, q);
        if byte_at(py, q) == Some(b'"') {
            q += 1;
        }
        if !byte_at(py, q).is_some_and(|b| b.is_ascii_digit()) {
            return None;
        }
        let n = run_while(py, q, is_digit_dot);
        Some(py[q..q + n].to_string())
    })
}

// `requires-python\s*=\s*"[^0-9]*([0-9.]+)` — 첫 숫자부터 [0-9.] 이어진 글(숫자가 없으면 None, 머리 주석 참고)
fn requires_python(t: &str) -> Option<String> {
    let lit = "requires-python";
    occurrences(t, lit, true).find_map(|p| {
        let w = skip_space(t, p + lit.len());
        if byte_at(t, w) != Some(b'=') {
            return None;
        }
        let q = skip_space(t, w + 1);
        if byte_at(t, q) != Some(b'"') {
            return None;
        }
        let rest = &t[q + 1..];
        let d = rest.bytes().position(|b| b.is_ascii_digit())?;
        let n = run_while(rest, d, is_digit_dot);
        Some(rest[d..d + n].to_string())
    })
}

// ICU 의 줄 끝 글자 — 멀티라인 `^` 는 이 글자 바로 뒤에서도 맞는다
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

// `(?m)^go\s+([0-9.]+)`
fn go_version(go: &str) -> Option<String> {
    go.char_indices().find_map(|(i, _)| {
        let at_line_start = go[..i].chars().next_back().is_none_or(is_line_terminator);
        if !at_line_start || !lit_at(go, i, "go", true) {
            return None;
        }
        let q = i + 2;
        let w = skip_space(go, q);
        if w == q {
            return None;
        }
        let n = run_while(go, w, is_digit_dot);
        (n > 0).then(|| go[w..w + n].to_string())
    })
}

// compose 의 `image:` 줄.
// version=true  : `image:\s*['"]?(?:[a-z0-9./-]*/)?<name><ql>:([0-9.]+)` (ql 이면 `(?:ql)?`) 의 판. 대소문자 무시
// version=false : `image:\s*['"]?(?:[a-z0-9./-]*/)?<name>` 가 있으면 Some(""). 대소문자를 가린다(원본 `range(of:options:.regularExpression)`)
// 이름 앞 경로 묶음은 `/` 로 끝나므로, 이름은 이어진 글자 덩어리(R)의 마지막 `/` 뒤 조각이다(판이 필요하면 바로 뒤가 `:` 이어야 하니까)
fn image_ref(t: &str, name: &str, ql: bool, version: bool) -> Option<String> {
    let ci = version;
    occurrences(t, "image:", ci).find_map(|p| {
        let mut q = skip_space(t, p + "image:".len());
        if is_quote(t, q) {
            q += 1;
        }
        let in_class = |b: u8| b.is_ascii_digit() || b.is_ascii_lowercase() || (ci && b.is_ascii_uppercase()) || matches!(b, b'.' | b'/' | b'-');
        let e = q + run_while(t, q, in_class);
        let r = &t[q..e];
        if version {
            let comp = r.rsplit('/').next().unwrap_or("");
            let ok = comp.eq_ignore_ascii_case(name) || (ql && comp.eq_ignore_ascii_case(&format!("{name}ql")));
            if !ok || byte_at(t, e) != Some(b':') {
                return None;
            }
            let n = run_while(t, e + 1, is_digit_dot);
            (n > 0).then(|| t[e + 1..e + 1 + n].to_string())
        } else {
            let boundary = |b: usize| b == 0 || r.as_bytes()[b - 1] == b'/';
            (0..r.len()).any(|b| boundary(b) && r[b..].starts_with(name)).then(String::new)
        }
    })
}

// `provider\s*=\s*"(postgresql|mysql)"` — 맞은 낱말(소문자로)
fn prisma_provider(t: &str) -> Option<&'static str> {
    let lit = "provider";
    occurrences(t, lit, true).find_map(|p| {
        let w = skip_space(t, p + lit.len());
        if byte_at(t, w) != Some(b'=') {
            return None;
        }
        let q = skip_space(t, w + 1);
        if byte_at(t, q) != Some(b'"') {
            return None;
        }
        ["postgresql", "mysql"].into_iter().find(|n| lit_at(t, q + 1, n, true) && byte_at(t, q + 1 + n.len()) == Some(b'"'))
    })
}

// ---------- 원본 비공개 보조 함수 ----------

// 앞뒤 빈칸을 깎고 `^~>=< v` 글자를 앞에서 떼고 `^([0-9]+(\.[0-9]+){0,2})` 만 남긴다. 없으면 ""
fn clean(v: Option<&str>) -> String {
    let Some(v) = v else { return String::new() };
    let t = v.trim_matches(is_cs_whitespace).trim_start_matches(|c| "^~>=< v".contains(c));
    let mut e = run_while(t, 0, |b| b.is_ascii_digit());
    if e == 0 {
        return String::new();
    }
    for _ in 0..2 {
        if byte_at(t, e) != Some(b'.') {
            break;
        }
        let d = run_while(t, e + 1, |b| b.is_ascii_digit());
        if d == 0 {
            break;
        }
        e += 1 + d;
    }
    t[..e].to_string()
}

// `"\(v)"` — 의존성 판이 글이 아닐 때의 글 꼴(머리 주석 참고)
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        Value::Null => "<null>".to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if !n.is_i64() && !n.is_u64() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

// package.json 의 의존성 판(dependencies·devDependencies·peerDependencies 순서로 처음 것). "%node" 는 engines.node
fn npm_deps(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(Value::Object(j)) = serde_json::from_str::<Value>(text) else { return out };
    for k in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(Value::Object(m)) = j.get(k) {
            for (n, v) in m {
                out.entry(n.clone()).or_insert_with(|| value_text(v));
            }
        }
    }
    if let Some(Value::Object(e)) = j.get("engines") {
        if let Some(n) = e.get("node") {
            out.insert("%node".to_string(), value_text(n));
        }
    }
    out
}

fn db_slot(key: &str, version: &str, source: String) -> StackSlot {
    let label = if key == "postgresql" { "PostgreSQL" } else { "MySQL" };
    StackSlot {
        key: key.to_string(),
        name: label.to_string(),
        framework: label.to_string(),
        framework_version: clean(Some(version)),
        origin: "detected".to_string(),
        sources: vec![source],
        ..Default::default()
    }
}

/// 읽은 파일(상대 경로 → 글)에서 칸 셋을 정한다. 칸마다 앞 폴더(뿌리 먼저)에서 처음 찾은 것. 원본: `apDetectStack(_ files:)`
pub fn detect_stack(files: &HashMap<String, String>) -> StackSet {
    let mut set = StackSet::default();
    for d in STACK_DIRS {
        let dir = Dir { files, d };
        let f = |n: &str| dir.get(n);
        // 백엔드: Spring Boot(Gradle·Maven), NestJS, FastAPI, Go, Express
        if set.back.is_none() {
            set.back = detect_back(&dir);
        }
        // 프론트: Next.js, Vue, React(Vite 면 그렇게)
        if set.front.is_none() {
            if let Some(pj) = f("package.json") {
                set.front = detect_front(d, pj);
            }
        }
        // DB: 설정 파일의 datasource, compose 이미지, prisma provider, 의존성
        if set.db.is_none() {
            set.db = detect_db(&dir);
        }
    }
    set
}

// 폴더 하나의 읽은 파일 — 원본의 지역 함수 `f(n) = files[d + n]`
struct Dir<'a> {
    files: &'a HashMap<String, String>,
    d: &'a str,
}

impl<'a> Dir<'a> {
    fn get(&self, n: &str) -> Option<&'a str> {
        self.files.get(&format!("{}{n}", self.d)).map(String::as_str)
    }
}

fn detect_back(dir: &Dir) -> Option<StackSlot> {
    let (d, f) = (dir.d, |n: &str| dir.get(n));
    let spring = |kotlin: bool, java: &str, boot: &str, build: &str, src: &str| StackSlot {
        key: if kotlin { "spring-kotlin" } else { "spring-java" }.to_string(),
        name: if kotlin { "Kotlin · Spring Boot" } else { "Java · Spring Boot" }.to_string(),
        lang: if kotlin { "Kotlin" } else { "Java" }.to_string(),
        lang_version: clean(Some(java)),
        framework: "Spring Boot".to_string(),
        framework_version: clean(Some(boot)),
        build: build.to_string(),
        origin: "detected".to_string(),
        sources: vec![format!("{d}{src}")],
    };
    let node = |key: &str, name: &str, lang: &str, lv: &str, fw: &str, fv: &str, build: &str, src: &str| StackSlot {
        key: key.to_string(),
        name: name.to_string(),
        lang: lang.to_string(),
        lang_version: clean(Some(lv)),
        framework: fw.to_string(),
        framework_version: clean(Some(fv)),
        build: build.to_string(),
        origin: "detected".to_string(),
        sources: vec![format!("{d}{src}")],
    };
    if let Some(g) = f("build.gradle.kts").or_else(|| f("build.gradle")).filter(|g| g.contains("org.springframework.boot")) {
        let kts = f("build.gradle.kts").is_some();
        let kotlin = g.contains("org.jetbrains.kotlin") || g.contains("kotlin(\"jvm\")") || g.contains("kotlin(\"plugin.spring\")");
        let boot = spring_boot_version(g);
        let java = call_number(g, "JavaLanguageVersion.of")
            .or_else(|| java_version_underscore(g).map(|v| v.replace('_', ".")))
            .or_else(|| source_compatibility(g))
            .or_else(|| call_number(g, "jvmToolchain"));
        return Some(spring(
            kotlin,
            java.as_deref().unwrap_or(""),
            boot.as_deref().unwrap_or(""),
            if kts { "Gradle Kotlin" } else { "Gradle Groovy" },
            if kts { "build.gradle.kts" } else { "build.gradle" },
        ));
    }
    if let Some(p) = f("pom.xml").filter(|p| p.contains("spring-boot")) {
        let boot = pom_parent_version(p);
        let java = tagged(p, "<java.version>", "</java.version>").or_else(|| tagged(p, "<maven.compiler.release>", "<"));
        let kotlin = p.contains("kotlin-maven-plugin");
        return Some(spring(kotlin, java.as_deref().unwrap_or(""), boot.as_deref().unwrap_or(""), "Maven", "pom.xml"));
    }
    if let Some((pj, nest)) = f("package.json").and_then(|pj| npm_deps(pj).get("@nestjs/core").cloned().map(|n| (pj, n))) {
        let deps = npm_deps(pj);
        return Some(node("nest", "TypeScript · NestJS", "Node.js", deps.get("%node").map_or("", String::as_str), "NestJS", &nest, "npm", "package.json"));
    }
    if let Some(py) = f("pyproject.toml").or_else(|| f("requirements.txt")).filter(|py| py.to_lowercase().contains("fastapi")) {
        let src = if f("pyproject.toml").is_some() { "pyproject.toml" } else { "requirements.txt" };
        let ver = fastapi_version(py);
        let pyv = requires_python(py);
        return Some(node(
            "fastapi",
            "Python · FastAPI",
            "Python",
            pyv.as_deref().unwrap_or(""),
            "FastAPI",
            ver.as_deref().unwrap_or(""),
            if src == "pyproject.toml" { "pyproject" } else { "pip" },
            src,
        ));
    }
    if let Some(go) = f("go.mod") {
        let fw = [("github.com/gin-gonic/gin", "Gin"), ("github.com/labstack/echo", "Echo"), ("github.com/gofiber/fiber", "Fiber")]
            .into_iter()
            .find(|(m, _)| go.contains(m))
            .map_or("", |(_, n)| n);
        let name = if fw.is_empty() { "Go".to_string() } else { format!("Go · {fw}") };
        return Some(StackSlot {
            name,
            lang: "Go".to_string(),
            lang_version: clean(go_version(go).as_deref()),
            framework: fw.to_string(),
            build: "go".to_string(),
            origin: "detected".to_string(),
            sources: vec![format!("{d}go.mod")],
            ..Default::default()
        });
    }
    if let Some((pj, ex)) = f("package.json").and_then(|pj| npm_deps(pj).get("express").cloned().map(|e| (pj, e))) {
        let deps = npm_deps(pj);
        return Some(node("", "Node.js · Express", "Node.js", deps.get("%node").map_or("", String::as_str), "Express", &ex, "npm", "package.json"));
    }
    None
}

// `<artifactId>spring-boot-starter-parent</artifactId>\s*<version>([^<]+)</version>`
fn pom_parent_version(p: &str) -> Option<String> {
    let lit = "<artifactId>spring-boot-starter-parent</artifactId>";
    occurrences(p, lit, true).find_map(|o| {
        let w = skip_space(p, o + lit.len());
        lit_at(p, w, "<version>", true).then(|| text_before_lt(p, w + "<version>".len(), "</version>")).flatten()
    })
}

fn detect_front(d: &str, pj: &str) -> Option<StackSlot> {
    let deps = npm_deps(pj);
    let has = |n: &str| deps.contains_key(n);
    let ver = |n: &str| clean(deps.get(n).map(String::as_str));
    let slot = |key: &str, name: &str, fw: &str, fv: String, build: &str| StackSlot {
        key: key.to_string(),
        name: name.to_string(),
        framework: fw.to_string(),
        framework_version: fv,
        build: build.to_string(),
        origin: "detected".to_string(),
        sources: vec![format!("{d}package.json")],
        ..Default::default()
    };
    if has("next") {
        Some(slot("next", "Next.js", "Next.js", ver("next"), "npm"))
    } else if has("vue") {
        Some(slot("vue", "Vue", "Vue", ver("vue"), if has("vite") { "Vite" } else { "npm" }))
    } else if has("react") && !has("react-native") {
        Some(slot("react", if has("vite") { "React (Vite)" } else { "React" }, "React", ver("react"), if has("vite") { "Vite" } else { "npm" }))
    } else {
        None
    }
}

fn detect_db(dir: &Dir) -> Option<StackSlot> {
    let (d, f) = (dir.d, |n: &str| dir.get(n));
    // (키, 판, 파일)
    let mut hit: Option<(&'static str, String, String)> = None;
    for n in ["src/main/resources/application.yml", "src/main/resources/application.yaml", "src/main/resources/application.properties"] {
        if hit.is_some() {
            continue;
        }
        let Some(t) = f(n) else { continue };
        if t.contains("jdbc:postgresql") {
            hit = Some(("postgresql", String::new(), n.to_string()));
        } else if t.contains("jdbc:mysql") || t.contains("jdbc:mariadb") {
            hit = Some(("mysql", String::new(), n.to_string()));
        }
    }
    for n in ["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"] {
        let Some(t) = f(n) else { continue };
        if let Some(v) = image_ref(t, "postgres", true, true) {
            if hit.as_ref().is_none_or(|h| h.0 == "postgresql") {
                let src = hit.as_ref().map_or_else(|| n.to_string(), |h| h.2.clone());
                hit = Some(("postgresql", v, src));
            }
        } else if let Some(v) = image_ref(t, "mysql", false, true) {
            if hit.as_ref().is_none_or(|h| h.0 == "mysql") {
                let src = hit.as_ref().map_or_else(|| n.to_string(), |h| h.2.clone());
                hit = Some(("mysql", v, src));
            }
        } else if hit.is_none() && image_ref(t, "postgres", true, false).is_some() {
            hit = Some(("postgresql", String::new(), n.to_string()));
        } else if hit.is_none() && image_ref(t, "mysql", false, false).is_some() {
            hit = Some(("mysql", String::new(), n.to_string()));
        }
    }
    if hit.is_none() {
        if let Some(t) = f("prisma/schema.prisma") {
            if let Some(pv) = prisma_provider(t) {
                hit = Some((pv, String::new(), "prisma/schema.prisma".to_string()));
            }
        }
    }
    if hit.is_none() {
        if let Some(pj) = f("package.json") {
            let deps = npm_deps(pj);
            if deps.contains_key("pg") {
                hit = Some(("postgresql", String::new(), "package.json".to_string()));
            } else if deps.contains_key("mysql2") || deps.contains_key("mysql") {
                hit = Some(("mysql", String::new(), "package.json".to_string()));
            }
        }
    }
    for n in ["pyproject.toml", "requirements.txt"] {
        if hit.is_some() {
            continue;
        }
        let Some(l) = f(n).map(str::to_lowercase) else { continue };
        if l.contains("psycopg") || l.contains("asyncpg") {
            hit = Some(("postgresql", String::new(), n.to_string()));
        } else if l.contains("pymysql") || l.contains("mysqlclient") || l.contains("aiomysql") {
            hit = Some(("mysql", String::new(), n.to_string()));
        }
    }
    hit.map(|h| db_slot(h.0, &h.1, format!("{d}{}", h.2)))
}

/// 저장소의 빌드·설정 파일 포트 — 규칙이 고른 상대 경로(`/` 구분자)만 그 뿌리에서 읽는다. 없거나 글이 아니면 `None`.
/// 앞부분만 읽는 크기 상한과 링크를 따라가지 않는 일은 어댑터 몫이다. 원본: `APStackFiles`(`Ports.swift`)
pub trait StackFiles: Send + Sync {
    fn read(&self, root: &str, relative: &str) -> Option<String>;
}

/// 유스케이스: 기존 저장소의 기술 스택 감지 — 고른 파일(`stack_candidates`)만 읽어(`StackFiles`) 칸 셋을 정한다.
/// 원본: `apDetectStack(root:files:)`
pub fn detect_stack_in_root(root: &str, files: &dyn StackFiles) -> StackSet {
    let got: HashMap<String, String> = stack_candidates().into_iter().filter_map(|rel| files.read(root, &rel).map(|t| (rel, t))).collect();
    detect_stack(&got)
}

// MARK: - 주제 글 낱말 대조

/// 찾은 낱말 하나 — 적힌 그대로(강조용), 어느 칸·무엇인지, 바로 뒤의 버전 꼴. 원본: `APTopicHit`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicHit {
    pub word: String,
    /// front·back·db·lang
    pub slot: String,
    /// 목록 항목 키 또는 언어(java·kotlin·typescript·python)
    pub target: String,
    pub version: String,
}

// 알려진 이름(설계 2장 표: Java·자바·Spring Boot·스프링·Kotlin·NestJS·네스트·FastAPI·React·리액트·Next.js·Vue·PostgreSQL·포스트그레스·MySQL 등).
// 긴 이름부터 본다(Spring Boot 를 Spring 보다 먼저). 한글 낱말은 뒤에 조사가 붙어도 찾고, 영문 낱말은 앞뒤가 영문 글자가 아닐 때만(JavaScript 의 Java 를 찾지 않게). 원본: `apTopicWords`
const TOPIC_WORDS: &[(&str, &str, &str)] = &[
    ("Spring Boot", "back", "spring"),
    ("SpringBoot", "back", "spring"),
    ("스프링 부트", "back", "spring"),
    ("스프링부트", "back", "spring"),
    ("스프링", "back", "spring"),
    ("Spring", "back", "spring"),
    ("NestJS", "back", "nest"),
    ("Nest.js", "back", "nest"),
    ("네스트JS", "back", "nest"),
    ("네스트", "back", "nest"),
    ("FastAPI", "back", "fastapi"),
    ("패스트API", "back", "fastapi"),
    ("Next.js", "front", "next"),
    ("NextJS", "front", "next"),
    ("넥스트JS", "front", "next"),
    ("넥스트", "front", "next"),
    ("React", "front", "react"),
    ("리액트", "front", "react"),
    ("Vue.js", "front", "vue"),
    ("Vue", "front", "vue"),
    ("PostgreSQL", "db", "postgresql"),
    ("Postgres", "db", "postgresql"),
    ("포스트그레스", "db", "postgresql"),
    ("포스트그레SQL", "db", "postgresql"),
    ("MySQL", "db", "mysql"),
    ("마이에스큐엘", "db", "mysql"),
    ("Kotlin", "lang", "kotlin"),
    ("코틀린", "lang", "kotlin"),
    ("Java", "lang", "java"),
    ("자바", "lang", "java"),
    ("TypeScript", "lang", "typescript"),
    ("타입스크립트", "lang", "typescript"),
    ("Python", "lang", "python"),
    ("파이썬", "lang", "python"),
];

// 자소 묶음을 앞 글자에 붙이는 확장 글자(머리 주석의 근사 목록)
fn is_extend(c: char) -> bool {
    matches!(
        c,
        '\u{300}'..='\u{36f}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{fe20}'..='\u{fe2f}'
            | '\u{200c}'
            | '\u{200d}'
            | '\u{1f3fb}'..='\u{1f3ff}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

// `c.map { $0.isASCII && $0.isLetter }` 중 글자 하나가 ASCII 영문인가
fn is_latin(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic())
}

// pos 에서 시작하는 `Character` 가 ASCII 영문 글자 하나인가(뒤에 결합 글자가 붙은 묶음은 ASCII 가 아니다)
fn latin_cluster_at(s: &str, pos: usize) -> bool {
    let mut it = s[pos..].chars();
    let first = it.next();
    is_latin(first) && !it.next().is_some_and(is_extend)
}

// 앞 16 글자까지 — `String(text[r.upperBound...].prefix(16))`. 원본은 `Character` 16 개지만 읽는 글(빈칸·v·숫자·점)은 모두 ASCII 이고
// 결합 글자가 붙으면 거기서 읽기가 끝나므로, 글자 묶음으로 세든 `char` 로 세든 잘리는 자리가 같다
fn prefix_16(s: &str) -> &str {
    s.char_indices().nth(16).map_or(s, |(i, _)| &s[..i])
}

// `^\s?v?([0-9]+(\.[0-9]+){0,2})(?![0-9])` 의 첫째 괄호. 없으면 ""
fn topic_version(tail: &str) -> String {
    let mut p = 0;
    if let Some(c) = tail.chars().next().filter(|&c| is_icu_space(c)) {
        p += c.len_utf8();
    }
    if matches!(byte_at(tail, p), Some(b'v' | b'V')) {
        p += 1;
    }
    let d = run_while(tail, p, |b| b.is_ascii_digit());
    if d == 0 {
        return String::new();
    }
    let mut e = p + d;
    for _ in 0..2 {
        if byte_at(tail, e) != Some(b'.') {
            break;
        }
        let d = run_while(tail, e + 1, |b| b.is_ascii_digit());
        if d == 0 {
            break;
        }
        e += 1 + d;
    }
    tail[p..e].to_string()
}

/// 주제 글에서 알려진 낱말을 찾는다(표 순서로 훑고, 겹친 자리는 먼저 잡은 쪽이 갖고, 글에 나온 차례로 돌려준다). 원본: `apTopicHits`
pub fn topic_hits(text: &str) -> Vec<TopicHit> {
    let mut hits: Vec<TopicHit> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for &(w, slot, target) in TOPIC_WORDS {
        let latin = w.as_bytes()[0].is_ascii(); // 낱말의 첫 글자가 ASCII
        let mut from = 0;
        while let Some(lo) = (from..text.len()).find(|&i| lit_at(text, i, w, true)) {
            let hi = lo + w.len();
            from = hi;
            if taken.iter().any(|&(a, b)| a < hi && lo < b) {
                continue;
            }
            if latin && (is_latin(text[..lo].chars().next_back()) || latin_cluster_at(text, hi)) {
                continue;
            }
            taken.push((lo, hi));
            // 바로 뒤(빈칸 하나까지)의 버전 꼴: 21, 4.1, 4.1.1, v18
            let version = topic_version(prefix_16(&text[hi..]));
            hits.push(TopicHit { word: text[lo..hi].to_string(), slot: slot.to_string(), target: target.to_string(), version });
        }
    }
    // 글에 나온 차례로(원본은 낱말 글의 처음 나온 자리로 정렬한다)
    hits.sort_by_key(|h| text.find(h.word.as_str()).unwrap_or(text.len()));
    hits
}

/// 주제 글에서 칸 셋을 미리 채운다(origin "topic"). 판은 글에 있으면 그것, 없으면 빈 칸(보드가 추천 판을 넣는다).
/// 백엔드: Spring 이면 언어 낱말이 Kotlin 일 때 Kotlin · Spring Boot, 아니면 Java · Spring Boot(언어 판은 Java·Kotlin 낱말 뒤의 판). 원본: `apTopicStack`
pub fn topic_stack(text: &str) -> StackSet {
    let hits = topic_hits(text);
    let mut set = StackSet::default();
    let lang = hits.iter().find(|h| h.slot == "lang" && (h.target == "java" || h.target == "kotlin"));
    if let Some(b) = hits.iter().find(|h| h.slot == "back") {
        set.back = Some(match b.target.as_str() {
            "spring" => {
                let kotlin = lang.is_some_and(|l| l.target == "kotlin");
                StackSlot {
                    key: if kotlin { "spring-kotlin" } else { "spring-java" }.to_string(),
                    name: if kotlin { "Kotlin · Spring Boot" } else { "Java · Spring Boot" }.to_string(),
                    lang: if kotlin { "Kotlin" } else { "Java" }.to_string(),
                    lang_version: lang.map_or_else(String::new, |l| l.version.clone()),
                    framework: "Spring Boot".to_string(),
                    framework_version: b.version.clone(),
                    build: "Gradle Groovy".to_string(),
                    origin: "topic".to_string(),
                    ..Default::default()
                }
            }
            "nest" => StackSlot {
                key: "nest".to_string(),
                name: "TypeScript · NestJS".to_string(),
                lang: "Node.js".to_string(),
                framework: "NestJS".to_string(),
                framework_version: b.version.clone(),
                build: "npm".to_string(),
                origin: "topic".to_string(),
                ..Default::default()
            },
            _ => {
                let py = hits.iter().find(|h| h.target == "python");
                StackSlot {
                    key: "fastapi".to_string(),
                    name: "Python · FastAPI".to_string(),
                    lang: "Python".to_string(),
                    lang_version: py.map_or_else(String::new, |p| p.version.clone()),
                    framework: "FastAPI".to_string(),
                    framework_version: b.version.clone(),
                    build: "pip".to_string(),
                    origin: "topic".to_string(),
                    ..Default::default()
                }
            }
        });
    }
    if let Some(f) = hits.iter().find(|h| h.slot == "front") {
        let pick = |react: &str, next: &str, vue: &str| match f.target.as_str() {
            "react" => react.to_string(),
            "next" => next.to_string(),
            "vue" => vue.to_string(),
            other => other.to_string(),
        };
        set.front = Some(StackSlot {
            key: f.target.clone(),
            name: pick("React (Vite)", "Next.js", "Vue"),
            framework: pick("React", "Next.js", "Vue"),
            framework_version: f.version.clone(),
            build: if f.target == "next" { "npm" } else { "Vite" }.to_string(),
            origin: "topic".to_string(),
            ..Default::default()
        });
    }
    if let Some(d) = hits.iter().find(|h| h.slot == "db") {
        let label = if d.target == "postgresql" { "PostgreSQL" } else { "MySQL" };
        set.db = Some(StackSlot {
            key: d.target.clone(),
            name: label.to_string(),
            framework: label.to_string(),
            framework_version: d.version.clone(),
            origin: "topic".to_string(),
            ..Default::default()
        });
    }
    set
}

/// 기존 저장소인데 주제 글이 다른 스택을 말할 때의 경고(설계 2장: 바꿀 수 없다, 스택 바꾸기는 그것을 주제로 한 따로 출발).
/// 감지한 칸과 같은 칸의 다른 것만 알린다(감지하지 못한 칸은 알리지 않는다 — 그 칸을 더하는 일이 주제일 수 있다). 원본: `apStackConflicts`
pub fn stack_conflicts(detected: &StackSet, topic: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in topic_hits(topic) {
        let slot = match h.slot.as_str() {
            "front" => detected.front.as_ref(),
            "back" => detected.back.as_ref(),
            "db" => detected.db.as_ref(),
            _ => {
                // 언어 낱말: 감지한 백엔드의 언어와 다르면(Java ↔ Kotlin 처럼)
                if let Some(b) = &detected.back {
                    let differs = !b.lang.is_empty()
                        && b.lang.to_lowercase() != h.target
                        && ["java", "kotlin", "python", "typescript"].contains(&h.target.as_str())
                        && !(h.target == "typescript" && b.lang == "Node.js");
                    if differs {
                        push_conflict(&mut out, &h.word, &b.lang);
                    }
                }
                continue;
            }
        };
        let Some(s) = slot else { continue };
        let same = match h.target.as_str() {
            "spring" => s.framework == "Spring Boot",
            "nest" => s.framework == "NestJS",
            "fastapi" => s.framework == "FastAPI",
            "react" => s.framework == "React",
            "next" => s.framework == "Next.js",
            "vue" => s.framework == "Vue",
            "postgresql" => s.framework == "PostgreSQL",
            "mysql" => s.framework == "MySQL",
            _ => true,
        };
        if !same {
            push_conflict(&mut out, &h.word, if s.framework.is_empty() { &s.name } else { &s.framework });
        }
    }
    out
}

// 같은 경고는 한 번만
fn push_conflict(out: &mut Vec<String>, word: &str, spec: &str) {
    let m = format!("주제에 적힌 '{word}': 이 프로젝트 규격({spec})과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다.");
    if !out.contains(&m) {
        out.push(m);
    }
}
