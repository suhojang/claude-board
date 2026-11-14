//! 결과 기록의 흐름 그림(result.json 의 `sequence[].mermaid`)을 읽는 규칙. 원본: `Sources/AutopilotRules/Sequence.swift`.
//! mermaid 의 `sequenceDiagram` 작은 부분집합만 읽고, 그 밖의 줄이 하나라도 있으면 그리지 않고(`Unsupported`) 보고서가 원문을 그대로 보인다.
//! 순수 함수만 있다(파일·시각·환경 없음).
//!
//! 이식된 선언(원본 순서): `SeqHead`(APSeqHead) · `SeqSide`(APSeqSide) · `SeqParticipant`(APSeqParticipant) · `SeqItem`(APSeqItem) ·
//! `SeqDiagram`(APSeqDiagram) · `SeqParse`(APSeqParse) · `SEQ_MAX_PARTICIPANTS`·`SEQ_MAX_ITEMS`·`SEQ_MAX_LABEL`(apSeqMax…) ·
//! 비공개 `ARROWS`·`BEGIN_KINDS`·`divide_of`·`first_word` · `parse_sequence`(apParseSequence) · `text_width`(apTextWidth) ·
//! `seq_lines`(apSeqLines) · `num`(비공개 `n`) · `sequence_svg`(apSequenceSVG) · 비공개 `arrow_head`(arrowHead).
//! SVG 쪽은 `rules::prose` 의 `plain`·`escape_html` 을 쓴다(복사하지 않는다). `seq_lines`·`num` 은 원본에서 비공개이고 시험 때문에 여기서만 공개다.
//!
//! 수 글자(`num`): 원본 `String(Int(x.rounded()))` — `.rounded()` 는 가장 가까운 정수, .5 는 0 에서 먼 쪽(Rust `f64::round` 와 같다).
//! 소수점이 없고(`1.0` 이 아니라 `1`), 음수는 `-` 가 붙고, `-0.4` 는 `0` 이다. NaN·무한대는 원본이 죽는 자리라 그리기 입력에 오지 않는다.
//!
//! Foundation 과 다른 점(원본 Swift 는 `Character`=자소 묶음 단위, 여기는 유니코드 글자(`char`) 단위):
//! - 길이 한도(`count` ≤ 80·300)와 `prefix(300)` 은 `chars()` 로 센다. 결합 글자·ZWJ 그림 글자·옛 한글 자모열은 원본이 하나로 세는 것을 여기서는 여럿으로 센다.
//! - `trimmingCharacters(in: .whitespaces)` 는 Zs 와 탭만 깎는다(`is_ws`). Rust 의 `char::is_whitespace` 와 다르다(줄바꿈 글자 제외).
//! - 원본의 `hasPrefix`·`range(of:)`·`firstIndex(of:)` 는 자소 묶음·정준 동치 기준이다. 여기는 바이트 열 그대로 견준다
//!   (화살표·` as `·`:` 바로 뒤에 결합 글자가 붙은 줄에서만 갈린다).

use std::collections::{BTreeSet, HashMap};

use super::prose::{escape_html, plain};

/// 메시지 화살표 머리. 원본: `APSeqHead`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqHead {
    None,
    Arrow,
    Cross,
    Open,
}

impl SeqHead {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            SeqHead::None => "none",
            SeqHead::Arrow => "arrow",
            SeqHead::Cross => "cross",
            SeqHead::Open => "open",
        }
    }
}

/// Note 의 자리. 원본: `APSeqSide`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqSide {
    Left,
    Right,
    Over,
}

impl SeqSide {
    /// 원본 `rawValue`
    pub fn as_str(self) -> &'static str {
        match self {
            SeqSide::Left => "left",
            SeqSide::Right => "right",
            SeqSide::Over => "over",
        }
    }
}

/// 참여자 한 명. 원본: `APSeqParticipant`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqParticipant {
    pub id: String,
    pub label: String,
    pub actor: bool,
}

/// 그림의 한 줄. 원본: `APSeqItem`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeqItem {
    Message { from: usize, to: usize, text: String, dotted: bool, head: SeqHead, both: bool },
    Note { at: Vec<usize>, side: SeqSide, text: String },
    /// loop·alt·opt·par·critical·break·rect·box
    Begin { kind: String, label: String },
    /// else·and·option
    Divide { kind: String, label: String },
    End,
}

/// 그림 값. 원본: `APSeqDiagram`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeqDiagram {
    pub title: String,
    pub participants: Vec<SeqParticipant>,
    pub items: Vec<SeqItem>,
    pub autonumber: bool,
}

/// 읽은 결과. 원본: `APSeqParse`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeqParse {
    Ok(SeqDiagram),
    /// 그리지 못한 첫 줄(1부터)과 까닭
    Unsupported { line: usize, why: String },
}

pub const SEQ_MAX_PARTICIPANTS: usize = 24;
pub const SEQ_MAX_ITEMS: usize = 300;
pub const SEQ_MAX_LABEL: usize = 300;

// 글자, 점선, 머리, 양쪽 — 긴 것부터
const ARROWS: [(&str, bool, SeqHead, bool); 10] = [
    ("<<-->>", true, SeqHead::Arrow, true),
    ("<<->>", false, SeqHead::Arrow, true),
    ("-->>", true, SeqHead::Arrow, false),
    ("--x", true, SeqHead::Cross, false),
    ("--)", true, SeqHead::Open, false),
    ("->>", false, SeqHead::Arrow, false),
    ("-->", true, SeqHead::None, false),
    ("-x", false, SeqHead::Cross, false),
    ("-)", false, SeqHead::Open, false),
    ("->", false, SeqHead::None, false),
];
const BEGIN_KINDS: [&str; 8] = ["loop", "alt", "opt", "par", "critical", "break", "rect", "box"];

fn divide_of(w: &str) -> Option<&'static str> {
    match w {
        "else" => Some("alt"),
        "and" => Some("par"),
        "option" => Some("critical"),
        _ => None,
    }
}

// Foundation `CharacterSet.whitespaces`: 일반 범주 Zs 와 탭(U+0009)
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn trim_ws(s: &str) -> &str {
    s.trim_matches(is_ws)
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

// 첫 빈칸·탭에서 둘로 나눈다. 빈칸이 없으면 (전부, "")
fn first_word(s: &str) -> (&str, &str) {
    match s.find([' ', '\t']) {
        None => (s, ""),
        Some(i) => (&s[..i], trim_ws(&s[i..])),
    }
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && char_count(s) <= 80 && !s.contains([' ', '\t', ':', ',', ';', '<', '>', '"'])
}

// `id as 이름` → (id, 이름). ` as ` 가 없으면 (전부, None)
fn split_as(s: &str) -> (&str, Option<&str>) {
    match s.find(" as ") {
        Some(p) => (trim_ws(&s[..p]), Some(trim_ws(&s[p + 4..]))),
        None => (s, None),
    }
}

fn participant(d: &mut SeqDiagram, index: &mut HashMap<String, usize>, id: &str, label: Option<&str>, actor: bool) -> Option<usize> {
    if let Some(&i) = index.get(id) {
        if let Some(l) = label {
            d.participants[i].label = l.to_string();
        }
        if actor {
            d.participants[i].actor = true;
        }
        return Some(i);
    }
    if d.participants.len() >= SEQ_MAX_PARTICIPANTS {
        return None;
    }
    d.participants.push(SeqParticipant { id: id.to_string(), label: label.unwrap_or(id).to_string(), actor });
    index.insert(id.to_string(), d.participants.len() - 1);
    Some(d.participants.len() - 1)
}

fn unsupported(line: usize, why: impl Into<String>) -> SeqParse {
    SeqParse::Unsupported { line, why: why.into() }
}

fn too_many() -> String {
    format!("참여자가 너무 많습니다({SEQ_MAX_PARTICIPANTS}까지)")
}

/// sequenceDiagram 원문 → 그림 값. 첫 의미 있는 줄은 `sequenceDiagram` 이어야 한다. 원본: `apParseSequence`
pub fn parse_sequence(src: &str) -> SeqParse {
    let mut d = SeqDiagram::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut started = false;
    let norm = src.replace("\r\n", "\n").replace('\r', "\n");
    for (k, raw) in norm.split('\n').enumerate() {
        let no = k + 1;
        let mut line = trim_ws(raw);
        if let Some(h) = line.strip_suffix(';') {
            line = trim_ws(h);
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if !started {
            if line != "sequenceDiagram" {
                return unsupported(no, "sequenceDiagram 으로 시작하지 않습니다");
            }
            started = true;
            continue;
        }
        if d.items.len() >= SEQ_MAX_ITEMS {
            return unsupported(no, format!("줄이 너무 많습니다({SEQ_MAX_ITEMS}줄까지 그립니다)"));
        }
        let (w0, rest) = first_word(line);
        let w = w0.to_lowercase();
        match w.as_str() {
            "participant" | "actor" => {
                let (id, label) = split_as(rest);
                if !valid_id(id) || label.map_or(0, char_count) > SEQ_MAX_LABEL {
                    return unsupported(no, "참여자 이름을 읽지 못했습니다");
                }
                if participant(&mut d, &mut index, id, label, w == "actor").is_none() {
                    return unsupported(no, too_many());
                }
                continue;
            }
            "create" => {
                let (w1, r1) = first_word(rest);
                let w1 = w1.to_lowercase();
                if w1 != "participant" && w1 != "actor" {
                    return unsupported(no, "읽지 못한 줄입니다");
                }
                let (id, label) = split_as(r1);
                if !valid_id(id) || participant(&mut d, &mut index, id, label, w1 == "actor").is_none() {
                    return unsupported(no, "참여자 이름을 읽지 못했습니다");
                }
                continue;
            }
            "autonumber" => {
                d.autonumber = true;
                continue;
            }
            "activate" | "deactivate" | "destroy" => continue,
            "title" | "title:" => {
                let t = trim_ws(rest.strip_prefix(':').unwrap_or(rest));
                d.title = t.chars().take(SEQ_MAX_LABEL).collect();
                continue;
            }
            "acctitle:" | "accdescr:" => continue,
            "end" => {
                if rest.is_empty() && stack.is_empty() {
                    return unsupported(no, "여는 묶음 없이 end 가 있습니다");
                }
                if rest.is_empty() {
                    stack.pop();
                    d.items.push(SeqItem::End);
                    continue;
                }
                // 뒤에 글이 있으면 묶음 끝이 아니라 메시지 줄로 읽어 본다
            }
            "note" => {
                let (side0, r1) = first_word(rest);
                let side = side0.to_lowercase();
                let (s, targets) = if side == "over" {
                    (SeqSide::Over, r1)
                } else if side == "left" || side == "right" {
                    let (of, r2) = first_word(r1);
                    if of.to_lowercase() != "of" {
                        return unsupported(no, "Note 줄을 읽지 못했습니다");
                    }
                    (if side == "left" { SeqSide::Left } else { SeqSide::Right }, r2)
                } else {
                    return unsupported(no, "Note 줄을 읽지 못했습니다");
                };
                let Some(colon) = targets.find(':') else {
                    return unsupported(no, "Note 줄에 쌍점(:)이 없습니다");
                };
                // Swift `split(separator:)` 는 빈 조각을 버린다(공백뿐인 조각은 남아 다듬은 뒤 valid_id 가 거른다)
                let names: Vec<&str> = targets[..colon].split(',').filter(|p| !p.is_empty()).map(trim_ws).collect();
                let text = trim_ws(&targets[colon + 1..]);
                if !(1..=2).contains(&names.len())
                    || !names.iter().all(|n| valid_id(n))
                    || char_count(text) > SEQ_MAX_LABEL
                    || !(s == SeqSide::Over || names.len() == 1)
                {
                    return unsupported(no, "Note 줄을 읽지 못했습니다");
                }
                let mut at = Vec::new();
                for n in names {
                    let Some(i) = participant(&mut d, &mut index, n, None, false) else {
                        return unsupported(no, too_many());
                    };
                    at.push(i);
                }
                d.items.push(SeqItem::Note { at, side: s, text: text.to_string() });
                continue;
            }
            _ => {
                if BEGIN_KINDS.contains(&w.as_str()) {
                    if char_count(rest) > SEQ_MAX_LABEL {
                        return unsupported(no, "묶음 이름이 너무 깁니다");
                    }
                    // rect·box 의 색 글자는 그리지 않는다
                    let label = if w == "rect" || w == "box" { String::new() } else { rest.to_string() };
                    d.items.push(SeqItem::Begin { kind: w.clone(), label });
                    stack.push(w);
                    continue;
                }
                if let Some(parent) = divide_of(&w) {
                    if stack.last().map(String::as_str) != Some(parent) {
                        return unsupported(no, format!("{w0} 가 맞는 묶음 안에 있지 않습니다"));
                    }
                    if char_count(rest) > SEQ_MAX_LABEL {
                        return unsupported(no, "묶음 이름이 너무 깁니다");
                    }
                    d.items.push(SeqItem::Divide { kind: w, label: rest.to_string() });
                    continue;
                }
            }
        }
        // 메시지: 보내는 쪽 화살표 받는 쪽: 글
        let mut hit: Option<(usize, usize, bool, SeqHead, bool)> = None;
        for (i, c) in line.char_indices() {
            for (tok, dotted, head, both) in ARROWS {
                if line[i..].starts_with(tok) {
                    hit = Some((i, i + tok.len(), dotted, head, both));
                    break;
                }
            }
            if hit.is_some() || c == ':' {
                break;
            }
        }
        let Some((hs, he, dotted, head, both)) = hit else {
            return unsupported(no, "읽지 못한 줄입니다");
        };
        let from = trim_ws(&line[..hs]);
        let mut after = trim_ws(&line[he..]);
        if after.starts_with('+') || after.starts_with('-') {
            after = trim_ws(&after[1..]);
        }
        let (to, text) = match after.find(':') {
            Some(c) => (trim_ws(&after[..c]), trim_ws(&after[c + 1..])),
            None => (after, ""),
        };
        if !valid_id(from) || !valid_id(to) || char_count(text) > SEQ_MAX_LABEL {
            return unsupported(no, "메시지 줄을 읽지 못했습니다");
        }
        let (Some(a), Some(b)) = (participant(&mut d, &mut index, from, None, false), participant(&mut d, &mut index, to, None, false)) else {
            return unsupported(no, too_many());
        };
        d.items.push(SeqItem::Message { from: a, to: b, text: text.to_string(), dotted, head, both });
    }
    if !started {
        return unsupported(1, "비어 있습니다");
    }
    if d.participants.is_empty() {
        return unsupported(1, "참여자가 없습니다");
    }
    // 닫지 않은 묶음은 끝에서 닫는다
    while stack.pop().is_some() {
        d.items.push(SeqItem::End);
    }
    SeqParse::Ok(d)
}

/// 글자 폭 어림(px) — 한글·한자·전각·그림 글자는 1em, 그 밖은 0.6em. 원본: `apTextWidth`
pub fn text_width(s: &str, size: f64) -> f64 {
    let mut w = 0.0;
    for c in s.chars() {
        let v = c as u32;
        let wide = (0x1100..=0x11FF).contains(&v)
            || (0x2E80..=0xA4CF).contains(&v)
            || (0xAC00..=0xD7A3).contains(&v)
            || (0xF900..=0xFAFF).contains(&v)
            || (0xFE30..=0xFE4F).contains(&v)
            || (0xFF00..=0xFF60).contains(&v)
            || (0xFFE0..=0xFFE6).contains(&v)
            || (0x1F300..=0x1FAFF).contains(&v);
        w += size * if wide { 1.0 } else { 0.6 };
    }
    w
}

// ICU 정규식 `\s` = [\t\n\f\r\p{Z}]. (Rust 의 `char::is_whitespace` 는 VT·U+0085 도 포함하므로 쓰지 않는다)
fn is_regex_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{c}' | '\r' | '\u{2028}' | '\u{2029}') || is_ws(c)
}

// `s[i..]` 가 `<br\s*/?>`(대소문자 무시)로 시작하면 그 끝 위치
fn br_end(s: &str, i: usize) -> Option<usize> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'<') || !matches!(b.get(i + 1), Some(b'b' | b'B')) || !matches!(b.get(i + 2), Some(b'r' | b'R')) {
        return None;
    }
    let mut j = i + 3;
    for c in s[j..].chars() {
        if !is_regex_space(c) {
            break;
        }
        j += c.len_utf8();
    }
    if b.get(j) == Some(&b'/') {
        j += 1;
    }
    (b.get(j) == Some(&b'>')).then_some(j + 1)
}

/// 줄바꿈 표지 `<br>`·`<br/>`·`<br />` 로 나눈 줄(각 줄은 `.whitespaces` 를 깎는다). 원본: `apSeqLines`(원본은 비공개, 시험 때문에 공개)
/// 나누기는 원본의 `components(separatedBy: "\n")`(NSString, UTF-16 단위)처럼 `\n` 에서만 자른다. `\r` 은 `.whitespaces` 가 아니라 남는다.
pub fn seq_lines(s: &str) -> Vec<String> {
    let mut replaced = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some(e) = br_end(s, i) {
            replaced.push('\n');
            i = e;
            continue;
        }
        let c = s[i..].chars().next().unwrap_or('\0');
        replaced.push(c);
        i += c.len_utf8();
    }
    let l: Vec<String> = replaced.split('\n').map(|p| trim_ws(p).to_string()).collect();
    if l.is_empty() {
        vec![String::new()]
    } else {
        l
    }
}

/// 수 → SVG 글자: 가장 가까운 정수(.5 는 0 에서 먼 쪽)로 반올림해 소수점 없이. 원본: 비공개 `n`(`String(Int(x.rounded()))`), 시험 때문에 공개
pub fn num(x: f64) -> String {
    (x.round() as i64).to_string()
}

// 글: apPlain(긴 줄표) → apEscapeHTML
fn esc(s: &str) -> String {
    escape_html(&plain(s))
}

fn lines_width(s: &str, size: f64) -> f64 {
    seq_lines(s).iter().map(|l| text_width(l, size)).fold(0.0, f64::max)
}

const LH: f64 = 16.0;

fn text_lines(lines: &[String], x: f64, top: f64, cls: &str, anchor: &str) -> String {
    let mut o = String::new();
    for (k, l) in lines.iter().enumerate() {
        o += &format!("<text class=\"{cls}\" x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\">{}</text>", num(x), num(top + k as f64 * LH + 12.0), esc(l));
    }
    o
}

// gap[lo..hi] 의 합이 w 보다 작으면 그 구간에 고르게 보탠다
fn need(gap: &mut [f64], lo: usize, hi: usize, w: f64) {
    if hi <= lo {
        return;
    }
    let have = gap[lo..hi].iter().fold(0.0, |a, b| a + b);
    if have < w {
        let add = (w - have) / (hi - lo) as f64;
        for g in &mut gap[lo..hi] {
            *g += add;
        }
    }
}

struct Open {
    kind: String,
    label: String,
    top: f64,
    used: BTreeSet<usize>,
    divides: Vec<(f64, String)>,
    depth: usize,
}

fn use_ps(open: &mut [Open], ps: &[usize]) {
    for o in open {
        o.used.extend(ps.iter().copied());
    }
}

/// 그림 값 → SVG 글자. 글은 `plain`(긴 줄표) → `escape_html` 을 거친다. label 은 화면 읽기 프로그램용 이름. 원본: `apSequenceSVG`
pub fn sequence_svg(d: &SeqDiagram, label: &str) -> String {
    let (fs, ms) = (13.0_f64, 12.5_f64);
    let p = &d.participants;
    let count = p.len();
    if count == 0 {
        return String::new();
    }
    let plines: Vec<Vec<String>> = p.iter().map(|q| seq_lines(&q.label)).collect();
    let box_w: Vec<f64> = p.iter().map(|q| f64::max(84.0, lines_width(&q.label, fs) + 24.0)).collect();
    let tallest = plines.iter().map(Vec::len).max().unwrap_or(1);
    let box_h = tallest as f64 * LH + 18.0;
    // 가로: 이웃 사이 간격 gap[i] = x[i+1]-x[i]. 메시지·Note 가 들어갈 만큼 넓힌다
    let mut gap: Vec<f64> = (0..count - 1).map(|k| f64::max(box_w[k] / 2.0 + box_w[k + 1] / 2.0 + 28.0, 120.0)).collect();
    let (mut left_extra, mut right_extra) = (0.0_f64, 0.0_f64);
    for it in &d.items {
        match it {
            SeqItem::Message { from: a, to: b, text, .. } => {
                let tw = lines_width(text, ms) + if d.autonumber { 22.0 } else { 0.0 };
                if a == b {
                    if *a < count - 1 {
                        need(&mut gap, *a, *a + 1, tw + 56.0);
                    } else {
                        right_extra = right_extra.max(tw + 56.0 - box_w[*a] / 2.0);
                    }
                } else {
                    need(&mut gap, *a.min(b), *a.max(b), tw + 32.0);
                }
            }
            SeqItem::Note { at, side, text } => {
                let w = f64::max(lines_width(text, ms) + 20.0, 70.0);
                match side {
                    SeqSide::Over if at.len() == 2 => need(&mut gap, at[0].min(at[1]), at[0].max(at[1]), w - 40.0),
                    SeqSide::Over => {}
                    SeqSide::Right => {
                        if at[0] < count - 1 {
                            need(&mut gap, at[0], at[0] + 1, w + 18.0);
                        } else {
                            right_extra = right_extra.max(w + 12.0 - box_w[at[0]] / 2.0);
                        }
                    }
                    SeqSide::Left => {
                        if at[0] > 0 {
                            need(&mut gap, at[0] - 1, at[0], w + 18.0);
                        } else {
                            left_extra = left_extra.max(w + 12.0 - box_w[0] / 2.0);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut x = vec![0.0_f64; count];
    x[0] = f64::max(box_w[0] / 2.0, 0.0) + 12.0 + left_extra.max(0.0);
    for k in 1..count {
        x[k] = x[k - 1] + gap[k - 1];
    }

    // 세로로 내려가며 그린다. 묶음 틀은 닫힐 때 그 안에서 쓴 참여자 폭으로
    let (mut body, mut frames) = (String::new(), String::new());
    let mut y = 10.0 + box_h + 18.0;
    let mut number = 0;
    let mut open: Vec<Open> = Vec::new();
    let mut min_x = 0.0_f64;
    let mut max_x = x[count - 1] + box_w[count - 1] / 2.0 + 12.0 + right_extra.max(0.0);
    for it in &d.items {
        match it {
            SeqItem::Message { from: a, to: b, text, dotted, head, both } => {
                let (a, b) = (*a, *b);
                use_ps(&mut open, &[a, b]);
                number += 1;
                let lines = if text.is_empty() { Vec::new() } else { seq_lines(text) };
                let cls = if *dotted { "ml d" } else { "ml" };
                let tag = if d.autonumber { format!("{number}. ") } else { String::new() };
                let top = y;
                let ly = y + lines.len() as f64 * LH + 6.0;
                let numbered = |lines: &[String]| {
                    let mut ls = if lines.is_empty() { vec![String::new()] } else { lines.to_vec() };
                    ls[0] = format!("{tag}{}", ls[0]);
                    ls
                };
                if a == b {
                    if !lines.is_empty() || d.autonumber {
                        body += &text_lines(&numbered(&lines), x[a] + 10.0, top, "mt", "start");
                        max_x = max_x.max(x[a] + 10.0 + lines_width(text, ms) + 30.0);
                    }
                    let r = x[a] + 40.0;
                    body += &format!(
                        "<polyline class=\"{cls}\" points=\"{},{} {},{} {},{} {},{}\"/>",
                        num(x[a]),
                        num(ly),
                        num(r),
                        num(ly),
                        num(r),
                        num(ly + 20.0),
                        num(x[a] + 2.0),
                        num(ly + 20.0)
                    );
                    body += &arrow_head(*head, x[a] + 2.0, ly + 20.0, -1.0);
                    max_x = max_x.max(r + 8.0);
                    y = ly + 20.0 + 16.0;
                } else {
                    let mid = (x[a] + x[b]) / 2.0;
                    if !lines.is_empty() || d.autonumber {
                        body += &text_lines(&numbered(&lines), mid, top, "mt", "middle");
                    }
                    let dir = if x[b] > x[a] { 1.0 } else { -1.0 };
                    let tip = x[b] - dir * 2.0;
                    body += &format!("<line class=\"{cls}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>", num(x[a]), num(ly), num(tip), num(ly));
                    body += &arrow_head(*head, tip, ly, dir);
                    if *both {
                        body += &arrow_head(SeqHead::Arrow, x[a] + dir * 2.0, ly, -dir);
                    }
                    y = ly + 16.0;
                }
            }
            SeqItem::Note { at, side, text } => {
                use_ps(&mut open, at);
                let lines = seq_lines(text);
                let w = f64::max(lines_width(text, ms) + 20.0, 70.0);
                let h = lines.len() as f64 * LH + 10.0;
                match side {
                    SeqSide::Over if at.len() == 2 => {
                        let (lo, hi) = (x[at[0]].min(x[at[1]]), x[at[0]].max(x[at[1]]));
                        let ww = f64::max(hi - lo + 40.0, w);
                        let left = (lo + hi) / 2.0 - ww / 2.0;
                        body += &format!("<rect class=\"nb\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\"/>", num(left), num(y), num(ww), num(h));
                        body += &text_lines(&lines, (lo + hi) / 2.0, y + 3.0, "nt", "middle");
                        min_x = min_x.min(left - 6.0);
                        max_x = max_x.max(left + ww + 6.0);
                    }
                    _ => {
                        let left = match side {
                            SeqSide::Over => x[at[0]] - w / 2.0,
                            SeqSide::Right => x[at[0]] + 10.0,
                            SeqSide::Left => x[at[0]] - 10.0 - w,
                        };
                        body += &format!("<rect class=\"nb\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\"/>", num(left), num(y), num(w), num(h));
                        body += &text_lines(&lines, left + w / 2.0, y + 3.0, "nt", "middle");
                        min_x = min_x.min(left - 6.0);
                        max_x = max_x.max(left + w + 6.0);
                    }
                }
                y += h + 12.0;
            }
            SeqItem::Begin { kind, label } => {
                let depth = open.len();
                open.push(Open { kind: kind.clone(), label: label.clone(), top: y, used: BTreeSet::new(), divides: Vec::new(), depth });
                y += if kind == "box" {
                    0.0
                } else if label.is_empty() {
                    14.0
                } else {
                    24.0
                };
            }
            SeqItem::Divide { label, .. } => {
                if let Some(o) = open.last_mut() {
                    o.divides.push((y, label.clone()));
                }
                y += 24.0;
            }
            SeqItem::End => {
                let Some(o) = open.pop() else { continue };
                if o.kind == "box" {
                    continue;
                }
                let pad = f64::max(56.0 - o.depth as f64 * 8.0, 24.0);
                let ps: BTreeSet<usize> = if o.used.is_empty() { (0..count).collect() } else { o.used };
                let lo = ps.iter().next().map_or(0.0, |&i| x[i]) - pad;
                let hi = ps.iter().next_back().map_or(0.0, |&i| x[i]) + pad;
                let bottom = y + 2.0;
                let mut f = format!(
                    "<rect class=\"fr{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>",
                    if o.kind == "rect" { " rc" } else { "" },
                    num(lo),
                    num(o.top),
                    num(hi - lo),
                    num(bottom - o.top)
                );
                if o.kind != "rect" {
                    let tw = text_width(&o.kind, 11.5) + 14.0;
                    f += &format!("<path class=\"ft\" d=\"M{} {}h{}v12l-6 6h-{}z\"/>", num(lo), num(o.top), num(tw), num(tw - 6.0));
                    f += &format!("<text class=\"fl\" x=\"{}\" y=\"{}\">{}</text>", num(lo + 6.0), num(o.top + 13.0), esc(&o.kind));
                    if !o.label.is_empty() {
                        f += &format!("<text class=\"fc\" x=\"{}\" y=\"{}\">[{}]</text>", num(lo + tw + 8.0), num(o.top + 14.0), esc(&o.label));
                    }
                }
                for (dy, dl) in &o.divides {
                    f += &format!("<line class=\"fs\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>", num(lo), num(*dy), num(hi), num(*dy));
                    if !dl.is_empty() {
                        f += &format!("<text class=\"fc\" x=\"{}\" y=\"{}\">[{}]</text>", num(lo + 8.0), num(dy + 15.0), esc(dl));
                    }
                }
                frames += &f;
                min_x = min_x.min(lo - 4.0);
                max_x = max_x.max(hi + 4.0);
                y = bottom + 10.0;
            }
        }
    }
    let bottom_top = y + 6.0;
    let (mut heads, mut lifes) = (String::new(), String::new());
    for (k, q) in p.iter().enumerate() {
        lifes += &format!("<line class=\"ll\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>", num(x[k]), num(10.0 + box_h), num(x[k]), num(bottom_top));
        for top in [10.0, bottom_top] {
            heads += &format!(
                "<rect class=\"pb{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>",
                if q.actor { " ac" } else { "" },
                num(x[k] - box_w[k] / 2.0),
                num(top),
                num(box_w[k]),
                num(box_h),
                if q.actor { 14 } else { 4 }
            );
            let ls = &plines[k];
            let first = top + (box_h - ls.len() as f64 * LH) / 2.0;
            heads += &text_lines(ls, x[k], first - 1.0, "pt", "middle");
        }
        min_x = min_x.min(x[k] - box_w[k] / 2.0 - 4.0);
    }
    let h = bottom_top + box_h + 10.0;
    let (vx, vw) = (min_x.floor(), (max_x - min_x).ceil());
    format!(
        "<svg class=\"sq\" viewBox=\"{} 0 {} {}\" width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"{}\">{frames}{lifes}{heads}{body}</svg>",
        num(vx),
        num(vw),
        num(h),
        num(vw),
        num(h),
        esc(label)
    )
}

fn arrow_head(head: SeqHead, tip_x: f64, y: f64, dir: f64) -> String {
    match head {
        SeqHead::None => String::new(),
        SeqHead::Arrow => format!(
            "<polygon class=\"ah\" points=\"{},{} {},{} {},{}\"/>",
            num(tip_x),
            num(y),
            num(tip_x - dir * 9.0),
            num(y - 4.5),
            num(tip_x - dir * 9.0),
            num(y + 4.5)
        ),
        SeqHead::Open => format!(
            "<polyline class=\"ml\" points=\"{},{} {},{} {},{}\"/>",
            num(tip_x - dir * 9.0),
            num(y - 5.0),
            num(tip_x),
            num(y),
            num(tip_x - dir * 9.0),
            num(y + 5.0)
        ),
        SeqHead::Cross => {
            let c = tip_x - dir * 5.0;
            format!(
                "<line class=\"ml\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/><line class=\"ml\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                num(c - 4.0),
                num(y - 4.0),
                num(c + 4.0),
                num(y + 4.0),
                num(c - 4.0),
                num(y + 4.0),
                num(c + 4.0),
                num(y - 4.0)
            )
        }
    }
}
