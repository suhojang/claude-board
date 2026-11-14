//! 바깥에 올릴 글의 형식 변환. 원본: `Sources/AutopilotRules/Formats.swift`(248줄).
//! 올릴 글은 받는 곳과 상관없는 작은 문서 꼴 `Doc`(제목·문단·목록·코드·가로줄, 글 조각은 굵게·https 링크)로 만들고,
//! 올릴 때 Jira 문서 형식(ADF)·Confluence 저장 형식(XHTML)·Notion 블록으로 바꾼다. 글은 모델이 쓴 기록에서 오므로 믿지 않는다:
//! 만들 때 다듬고(`doc_text`) 저장 형식은 모든 글을 이스케이프하며 링크는 https 만(`safe_link`) 건다.
//!
//! 이식된 선언(원본 순서): `Span`(APSpan) · `Block`(APBlock) · `Doc`(APDoc) · 상수 여섯(`DOC_MARKER` …
//! `NOTION_BATCH_MAX`) · `doc_text`(apDocText) · `doc_code`(apDocCode) · `doc_clamp`(apDocClamp) · `adf`(apADF) · `storage`(apStorage) ·
//! `notion_rich`(apNotionRich) · 비공개 `notion_spans`·`notion_block` · `notion_blocks`(apNotionBlocks) · `batches`(apBatches) ·
//! `doc_json`(apDocJSON) · `doc_parse`(apDocParse) · `doc_plain_lines`(apDocPlainLines).
//!
//! Swift ↔ Rust 로 달라진 점:
//! - 글자 수·자르기는 Swift 의 `Character`(확장 자소 덩어리) 기준이다. 이 크레이트에는 `unicode-segmentation` 이 없어(Cargo 는 통합자 몫)
//!   `grapheme_len` 이 UAX #29 의 주요 규칙(CRLF·제어·결합 표시·ZWJ 이모지·국기 쌍·한글 자모)만 옮겼다. 그 밖의 드문 덩어리(Prepend·인도계 결합 등)는 다르게 셀 수 있다.
//! - 가장자리 빈칸(`.whitespaces`)은 Zs 범주와 탭이다(`ws`). `char::is_whitespace` 가 아니다.
//! - ADF 는 `serde_json::Value`(Swift `[String: Any]`)다. 이 크레이트는 `preserve_order` 라 키 순서가 만든 순서다 — Swift 사전은 순서가 없어 시험이 키를 정렬해 견준다.
//! - 제목 수준은 `i64` 다(Swift `Int`). 기본 인자는 Rust 에 없어 `doc_text`·`doc_code` 는 기본 상한, `doc_text_n`·`doc_code_n` 은 상한을 받는다.
//! - `doc_code` 는 `\n` 으로만 줄을 가른다. Swift `components(separatedBy: "\n")` 는 Foundation 의 글 찾기라 `\r\n` 안의 `\n` 에서도 가르고(`\r` 은 앞 줄 끝에 남아
//!   `doc_text` 가 빈칸으로 바꿔 지운다), Rust 는 같은 결과를 낸다. Swift 시험은 CRLF 를 못박지 않았고 이 머신에서 Swift 를 못 돌려 Foundation 쪽은 확인하지 못했다(`extra_코드_칸_crlf`).
//! - `doc_parse` 는 Swift `Any?`(JSONSerialization 결과) 대신 `Option<&serde_json::Value>` 를 받는다. 모양이 틀린 칸은 Swift 와 같이 빈 값으로 본다:
//!   배열 원소 하나라도 사전이 아니면 `as? [[String: Any]]` 가 통째로 실패해 빈 배열이다. 불리언은 JSON `true`·`false` 만 받는다
//!   (Swift `as? Bool` 은 NSNumber 0·1 도 받을 수 있으나 이 머신에서 확인하지 못했다). 수준 `n` 은 NSNumber `intValue` 처럼 소수는 버리고 불리언은 1·0 이다.
//! - `batches` 는 `T: Clone` 이다(Swift 는 값 복사).

use crate::rules::prose::{escape_html, plain, safe_link};
use serde_json::{json, Map, Value};
use unicode_normalization::char::is_combining_mark;

/// 글 조각 — `Text(글, 굵게)` 또는 `Link(글, 주소)`. 주소는 `safe_link` 를 지난 https 주소. 원본: `APSpan`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String, bool),
    Link(String, String),
}

/// 문서 블록. 원본: `APBlock`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 수준 1~3
    Heading(i64, String),
    Paragraph(Vec<Span>),
    /// 줄마다 조각들
    Bullets(Vec<Vec<Span>>),
    Code(String),
    Rule,
}

/// 작은 문서 꼴. 원본: `APDoc`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

impl Doc {
    pub fn new(blocks: Vec<Block>) -> Doc {
        Doc { blocks }
    }
}

/// 바깥 글 첫 줄(사용자 확정). 원본: `apDocMarker`
pub const DOC_MARKER: &str = "오토파일럿 작성";
/// 글 조각 하나의 글자 수 상한. 원본: `apDocTextMax`
pub const DOC_TEXT_MAX: usize = 4000;
/// 문서 하나의 블록 수 상한. 원본: `apDocBlockMax`
pub const DOC_BLOCK_MAX: usize = 400;
/// 목록 하나의 줄 수 상한. 원본: `apDocListMax`
pub const DOC_LIST_MAX: usize = 200;
/// Notion 글 한 칸(rich_text 하나)의 글자 수 상한. 원본: `apNotionTextMax`
pub const NOTION_TEXT_MAX: usize = 2000;
/// Notion 에 한 번에 보내는 블록 수 상한. 원본: `apNotionBatchMax`
pub const NOTION_BATCH_MAX: usize = 100;

// ---- 확장 자소 덩어리(Swift Character) 세기 ----

fn is_control(c: char) -> bool {
    let v = c as u32;
    v < 0x20 || (0x7F..=0x9F).contains(&v) || v == 0x2028 || v == 0x2029
}

fn is_extend(c: char) -> bool {
    let v = c as u32;
    is_combining_mark(c) || v == 0x200C || v == 0x200D || (0x1F3FB..=0x1F3FF).contains(&v) || (0xE0020..=0xE007F).contains(&v)
}

fn is_pict(c: char) -> bool {
    let v = c as u32;
    (0x1F000..=0x1FAFF).contains(&v)
        || (0x2600..=0x27BF).contains(&v)
        || (0x2300..=0x23FF).contains(&v)
        || (0x2B00..=0x2BFF).contains(&v)
        || matches!(v, 0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139)
}

fn is_ri(c: char) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&(c as u32))
}

// 1 L, 2 V, 3 T, 4 LV, 5 LVT, 0 그 밖
fn hangul(c: char) -> u8 {
    let v = c as u32;
    match v {
        0x1100..=0x115F | 0xA960..=0xA97C => 1,
        0x1160..=0x11A7 | 0xD7B0..=0xD7C6 => 2,
        0x11A8..=0x11FF | 0xD7CB..=0xD7FB => 3,
        0xAC00..=0xD7A3 => {
            if (v - 0xAC00).is_multiple_of(28) {
                4
            } else {
                5
            }
        }
        _ => 0,
    }
}

// 글 맨 앞 덩어리의 바이트 길이(글은 비어 있지 않다)
fn first_cluster_len(s: &str) -> usize {
    let mut it = s.char_indices();
    let Some((_, first)) = it.next() else { return 0 };
    let (mut prev, mut ri) = (first, usize::from(is_ri(first)));
    let (mut pict_open, mut zwj_pending) = (is_pict(first), false);
    for (i, c) in it {
        let (p, q) = (hangul(prev), hangul(c));
        let free = !is_control(prev) && !is_control(c); // 제어 글자 앞뒤는 늘 끊긴다(CR LF 만 예외)
        let join = (prev == '\r' && c == '\n')
            || (free
                && (is_extend(c)
                    || (prev == '\u{200D}' && zwj_pending && is_pict(c))
                    || (is_ri(prev) && is_ri(c) && ri % 2 == 1)
                    || (p == 1 && matches!(q, 1 | 2 | 4 | 5))
                    || (matches!(p, 4 | 2) && matches!(q, 2 | 3))
                    || (matches!(p, 5 | 3) && q == 3)));
        if !join {
            return i;
        }
        zwj_pending = if c == '\u{200D}' { pict_open } else { zwj_pending && is_extend(c) };
        if !is_extend(c) && !(prev == '\u{200D}' && is_pict(c)) {
            pict_open = is_pict(c);
        }
        ri = if is_ri(c) { ri + 1 } else { 0 };
        prev = c;
    }
    s.len()
}

// 덩어리가 n 개보다 많으면 n 개째 끝의 바이트 위치, 아니면 None
fn cut_after(s: &str, n: usize) -> Option<usize> {
    if s.len() <= n {
        return None; // 덩어리는 바이트보다 많을 수 없다
    }
    let mut at = 0;
    for _ in 0..n {
        at += first_cluster_len(&s[at..]);
    }
    (at < s.len()).then_some(at)
}

/// Swift `Character` 수 근사. 시험용으로 공개한다(상한 판정과 같은 규칙).
pub fn grapheme_len(s: &str) -> usize {
    let (mut at, mut n) = (0, 0);
    while at < s.len() {
        at += first_cluster_len(&s[at..]);
        n += 1;
    }
    n
}

// CharacterSet.whitespaces — Zs 범주와 탭
fn ws(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// 글 다듬기(기본 상한 4000, 가장자리 빈칸 지움). 원본: `apDocText`
pub fn doc_text(s: &str) -> String {
    doc_text_n(s, DOC_TEXT_MAX, true)
}

/// 글 다듬기 — 긴 줄표는 쉼표로(`plain`), 줄바꿈·탭은 빈칸으로, 화면을 속이는 글자(제어·방향 바꾸기·줄 구분자)는 지우고,
/// 상한 `n` 글자에서 자르고 `…` 를 붙인다. `trim` 이 거짓이면 앞뒤 빈칸을 남긴다(글 조각 사이 띄어쓰기). 원본: `apDocText`
pub fn doc_text_n(s: &str, n: usize, trim: bool) -> String {
    let mut o = String::new();
    for u in plain(s).chars() {
        let v = u as u32;
        if matches!(u, '\n' | '\t' | '\r') || v == 0x2028 || v == 0x2029 {
            o.push(' ');
            continue;
        }
        if v < 0x20 || (0x7F..=0x9F).contains(&v) || (0x202A..=0x202E).contains(&v) || (0x2066..=0x2069).contains(&v) || v == 0xFEFF {
            continue;
        }
        o.push(u);
    }
    // ` {2,}` → ` `
    let mut c = String::with_capacity(o.len());
    let mut prev_space = false;
    for ch in o.chars() {
        if ch == ' ' && prev_space {
            continue;
        }
        prev_space = ch == ' ';
        c.push(ch);
    }
    let t = if trim { c.trim_matches(ws) } else { c.as_str() };
    match cut_after(t, n) {
        Some(at) => format!("{}…", &t[..at]),
        None => t.to_string(),
    }
}

/// 코드 칸(기본 상한). 원본: `apDocCode`
pub fn doc_code(s: &str) -> String {
    doc_code_n(s, DOC_TEXT_MAX)
}

/// 코드 칸 — 줄바꿈은 남기고 나머지는 `doc_text` 와 같게, 전체를 `n` 글자에서 자른다(`…` 없이). 원본: `apDocCode`
pub fn doc_code_n(s: &str, n: usize) -> String {
    let joined = s.split('\n').map(|l| doc_text_n(l, n, true)).collect::<Vec<_>>().join("\n");
    match cut_after(&joined, n) {
        Some(at) => joined[..at].to_string(),
        None => joined,
    }
}

fn clamp_spans(a: &[Span]) -> Vec<Span> {
    a.iter()
        .filter_map(|s| match s {
            Span::Text(t, b) => {
                let x = doc_text_n(t, DOC_TEXT_MAX, false);
                if x.trim_matches(ws).is_empty() {
                    None
                } else {
                    Some(Span::Text(x, *b))
                }
            }
            Span::Link(t, u) => {
                let x = doc_text_n(t, 300, false);
                match safe_link(u) {
                    None => (!x.is_empty()).then_some(Span::Text(x, false)),
                    Some(ok) => Some(Span::Link(if x.is_empty() { ok.clone() } else { x }, ok)),
                }
            }
        })
        .collect()
}

/// 문서 다듬기 — 블록 수·목록 줄 수 상한, 빈 조각 빼기, 링크 다시 확인(만드는 쪽이 이미 했어도 한 번 더). 원본: `apDocClamp`
pub fn doc_clamp(d: &Doc) -> Doc {
    let mut out = Vec::new();
    for b in d.blocks.iter().take(DOC_BLOCK_MAX) {
        match b {
            Block::Heading(l, t) => {
                let x = doc_text_n(t, 300, true);
                if !x.is_empty() {
                    out.push(Block::Heading((*l).clamp(1, 3), x));
                }
            }
            Block::Paragraph(a) => {
                let s = clamp_spans(a);
                if !s.is_empty() {
                    out.push(Block::Paragraph(s));
                }
            }
            Block::Bullets(rows) => {
                let r: Vec<Vec<Span>> = rows.iter().take(DOC_LIST_MAX).map(|r| clamp_spans(r)).filter(|r| !r.is_empty()).collect();
                if !r.is_empty() {
                    out.push(Block::Bullets(r));
                }
            }
            Block::Code(t) => {
                let x = doc_code(t);
                if !x.is_empty() {
                    out.push(Block::Code(x));
                }
            }
            Block::Rule => out.push(Block::Rule),
        }
    }
    Doc::new(out)
}

// ---- Jira 문서 형식(ADF) ----

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

fn adf_spans(a: &[Span]) -> Vec<Value> {
    a.iter()
        .filter_map(|s| match s {
            // ADF 는 빈 글 마디를 받지 않는다
            Span::Text(t, b) => {
                if t.is_empty() {
                    return None;
                }
                let mut n = obj(vec![("type", json!("text")), ("text", json!(t))]);
                if *b {
                    n["marks"] = json!([{"type": "strong"}]);
                }
                Some(n)
            }
            Span::Link(t, u) => {
                if t.is_empty() {
                    return None;
                }
                Some(json!({"type": "text", "text": t, "marks": [{"type": "link", "attrs": {"href": u}}]}))
            }
        })
        .collect()
}

/// Jira 문서 형식(ADF) 으로. 먼저 `doc_clamp` 를 지난다. 원본: `apADF`
pub fn adf(d0: &Doc) -> Value {
    let d = doc_clamp(d0);
    let mut content: Vec<Value> = Vec::new();
    for b in &d.blocks {
        match b {
            Block::Heading(l, t) => content.push(json!({"type": "heading", "attrs": {"level": l}, "content": [{"type": "text", "text": t}]})),
            Block::Paragraph(a) => content.push(json!({"type": "paragraph", "content": adf_spans(a)})),
            Block::Bullets(rows) => {
                let items: Vec<Value> = rows
                    .iter()
                    .map(|r| json!({"type": "listItem", "content": [{"type": "paragraph", "content": adf_spans(r)}]}))
                    .collect();
                content.push(json!({"type": "bulletList", "content": items}));
            }
            Block::Code(t) => content.push(json!({"type": "codeBlock", "content": [{"type": "text", "text": t}]})),
            Block::Rule => content.push(json!({"type": "rule"})),
        }
    }
    json!({"type": "doc", "version": 1, "content": content})
}

// ---- Confluence 저장 형식(XHTML) ----

fn storage_spans(a: &[Span]) -> String {
    a.iter()
        .map(|s| match s {
            Span::Text(t, b) => {
                if *b {
                    format!("<strong>{}</strong>", escape_html(t))
                } else {
                    escape_html(t)
                }
            }
            Span::Link(t, u) => format!("<a href=\"{}\">{}</a>", escape_html(u), escape_html(t)),
        })
        .collect()
}

/// Confluence 저장 형식으로. 매크로(`ac:…`)·CDATA 를 쓰지 않는다 — 글에 든 `]]>` 나 매크로 꼴이 그대로 글로 남게. 코드는 `<pre>`.
/// 먼저 `doc_clamp` 를 지난다. 원본: `apStorage`
pub fn storage(d0: &Doc) -> String {
    let d = doc_clamp(d0);
    let mut o = String::new();
    for b in &d.blocks {
        match b {
            Block::Heading(l, t) => o += &format!("<h{l}>{}</h{l}>", escape_html(t)),
            Block::Paragraph(a) => o += &format!("<p>{}</p>", storage_spans(a)),
            Block::Bullets(rows) => {
                o += "<ul>";
                for r in rows {
                    o += &format!("<li>{}</li>", storage_spans(r));
                }
                o += "</ul>";
            }
            Block::Code(t) => o += &format!("<pre>{}</pre>", escape_html(t)),
            Block::Rule => o += "<hr />",
        }
    }
    o
}

// ---- Notion 블록 ----

/// 글 하나를 rich_text 칸들로 — 칸마다 2,000자 이하(Notion 상한). 유니코드 글자(확장 자소) 경계에서 자른다. 원본: `apNotionRich`
pub fn notion_rich(s: &str, bold: bool, link: Option<&str>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let at = cut_after(rest, NOTION_TEXT_MAX).unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(at);
        rest = tail;
        let mut text = obj(vec![("content", json!(piece))]);
        if let Some(url) = link {
            text["link"] = json!({"url": url});
        }
        let mut item = obj(vec![("type", json!("text")), ("text", text)]);
        if bold {
            item["annotations"] = json!({"bold": true});
        }
        out.push(item);
    }
    out
}

// 블록 하나의 rich_text 는 100칸까지(Notion 상한)
fn notion_spans(a: &[Span]) -> Vec<Value> {
    a.iter()
        .flat_map(|s| match s {
            Span::Text(t, b) => notion_rich(t, *b, None),
            Span::Link(t, u) => notion_rich(t, false, Some(u)),
        })
        .take(100)
        .collect()
}

fn notion_block(kind: &str, rich: Vec<Value>, extra: Vec<(&str, Value)>) -> Value {
    let mut inner = obj(vec![("rich_text", Value::Array(rich))]);
    for (k, v) in extra {
        inner[k] = v;
    }
    obj(vec![("object", json!("block")), ("type", json!(kind)), (kind, inner)])
}

/// Notion 블록 목록으로. 먼저 `doc_clamp` 를 지난다. 목록 줄은 블록 하나씩이다. 원본: `apNotionBlocks`
pub fn notion_blocks(d0: &Doc) -> Vec<Value> {
    let d = doc_clamp(d0);
    let mut out = Vec::new();
    for b in &d.blocks {
        match b {
            Block::Heading(l, t) => out.push(notion_block(&format!("heading_{l}"), notion_rich(t, false, None), vec![])),
            Block::Paragraph(a) => out.push(notion_block("paragraph", notion_spans(a), vec![])),
            Block::Bullets(rows) => out.extend(rows.iter().map(|r| notion_block("bulleted_list_item", notion_spans(r), vec![]))),
            Block::Code(t) => out.push(notion_block("code", notion_rich(t, false, None), vec![("language", json!("plain text"))])),
            Block::Rule => out.push(json!({"object": "block", "type": "divider", "divider": {}})),
        }
    }
    out
}

/// 한 번에 보낼 묶음(기본 100개씩). 원본: `apBatches`
pub fn batches<T: Clone>(a: &[T]) -> Vec<Vec<T>> {
    batches_n(a, NOTION_BATCH_MAX)
}

/// 한 번에 보낼 묶음 — `n` 개씩(Notion: 처음 묶음은 페이지 만들기와 함께, 나머지는 블록 덧붙이기로). `n` 이 0 이면 통째로 한 묶음. 원본: `apBatches`
pub fn batches_n<T: Clone>(a: &[T], n: usize) -> Vec<Vec<T>> {
    if n == 0 {
        return vec![a.to_vec()];
    }
    a.chunks(n).map(<[T]>::to_vec).collect()
}

// ---- 문서 ↔ 파일 칸(outbox 글 파일의 doc) ----

fn span_json(a: &[Span]) -> Value {
    Value::Array(
        a.iter()
            .map(|s| match s {
                Span::Text(t, true) => json!({"x": t, "b": true}),
                Span::Text(t, false) => json!({"x": t}),
                Span::Link(t, u) => json!({"x": t, "l": u}),
            })
            .collect(),
    )
}

/// 문서 → 파일 칸(블록마다 사전 하나). 원본: `apDocJSON`
pub fn doc_json(d: &Doc) -> Vec<Value> {
    d.blocks
        .iter()
        .map(|b| match b {
            Block::Heading(l, t) => json!({"t": "h", "n": l, "x": t}),
            Block::Paragraph(a) => json!({"t": "p", "s": span_json(a)}),
            Block::Bullets(r) => json!({"t": "ul", "i": r.iter().map(|x| span_json(x)).collect::<Vec<_>>()}),
            Block::Code(t) => json!({"t": "code", "x": t}),
            Block::Rule => json!({"t": "hr"}),
        })
        .collect()
}

// Swift `as? [[String: Any]]` — 배열이고 원소가 모두 사전일 때만. 아니면 None(호출 쪽이 빈 배열로 본다)
fn dict_array(v: Option<&Value>) -> Option<Vec<&Map<String, Value>>> {
    v?.as_array()?.iter().map(Value::as_object).collect()
}

fn parse_spans(v: Option<&Value>) -> Vec<Span> {
    dict_array(v)
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            let x = e.get("x").and_then(Value::as_str).unwrap_or("").to_string();
            match e.get("l").and_then(Value::as_str) {
                Some(u) => Span::Link(x, u.to_string()),
                None => Span::Text(x, e.get("b").and_then(Value::as_bool).unwrap_or(false)),
            }
        })
        .collect()
}

// NSNumber.intValue — 수는 소수를 버리고 불리언은 1·0, 그 밖은 None
fn int_value(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// 파일 칸 → 문서(모르는 블록은 버리고 다듬기를 다시 한다). 원본: `apDocParse`
pub fn doc_parse(v: Option<&Value>) -> Doc {
    let blocks: Vec<Block> = dict_array(v)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| match e.get("t").and_then(Value::as_str) {
            Some("h") => Some(Block::Heading(int_value(e.get("n")).unwrap_or(2), e.get("x").and_then(Value::as_str).unwrap_or("").to_string())),
            Some("p") => Some(Block::Paragraph(parse_spans(e.get("s")))),
            Some("ul") => Some(Block::Bullets(e.get("i").and_then(Value::as_array).map(|rows| rows.iter().map(|r| parse_spans(Some(r))).collect()).unwrap_or_default())),
            Some("code") => Some(Block::Code(e.get("x").and_then(Value::as_str).unwrap_or("").to_string())),
            Some("hr") => Some(Block::Rule),
            _ => None,
        })
        .collect();
    doc_clamp(&Doc::new(blocks))
}

fn span_plain(a: &[Span]) -> String {
    a.iter()
        .map(|s| match s {
            Span::Text(t, _) | Span::Link(t, _) => t.as_str(),
        })
        .collect()
}

/// 문서를 쉬운 글로(검수 화면의 한 줄 요약, 시험) — 블록마다 한 줄. 원본: `apDocPlainLines`
pub fn doc_plain_lines(d: &Doc) -> Vec<String> {
    d.blocks
        .iter()
        .flat_map(|b| match b {
            Block::Heading(_, t) | Block::Code(t) => vec![t.clone()],
            Block::Paragraph(a) => vec![span_plain(a)],
            Block::Bullets(r) => r.iter().map(|x| format!("· {}", span_plain(x))).collect(),
            Block::Rule => vec!["(가로줄)".to_string()],
        })
        .collect()
}
