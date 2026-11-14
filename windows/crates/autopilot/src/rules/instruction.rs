//! 자율주행 지시(세트 `private\projects\<프로젝트>\set\autopilot.md`)를 그 자리에서 고치기와 지난 판(기록 저장소 sets 의 instruction)과의 차이.
//! 원본: `Sources/AutopilotRules/Instruction.swift`. 기록 저장소·파일은 보드가 읽고 쓰고, 이 규칙은 글 두 개를 받는 순수 함수다.
//! 원본의 `APDiffKind`·`APDiffLine`·`apLineDiff`·`apDiffCount`·`apInstructionHead`·`apInstructionSaveProblem` 가
//! `DiffKind`·`DiffLine`·`line_diff`·`diff_count`·`instruction_head`·`instruction_save_problem` 이다.
//!
//! Foundation 과 다른 점(정직한 한계):
//! - 줄이 같은지는 Swift `String ==`(정규 등가) 처럼 NFC 로 바꿔 견준다. 돌려주는 글은 원문 그대로다.
//! - 길이는 Swift `String.count`(문자 = 자소 묶음) 대신 유니코드 글자(스칼라) 수에서 `\r\n` 한 쌍을 한 글자로 센 값이다.
//!   결합 글자·ZWJ 이모지가 든 10만 자 언저리의 글에서만 원본과 한 글자쯤 어긋날 수 있다(자소 분리 크레이트가 없다).
//! - 앞뒤 빈칸 자르기는 `trim()`(Unicode White_Space)이며 Foundation 의 `.whitespacesAndNewlines` 와 같은 글자 집합이다.

use unicode_normalization::UnicodeNormalization;

/// 줄 하나가 같은지·뺀 것인지·더한 것인지. 원본: `APDiffKind`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Add,
    Del,
}

/// 차이의 한 줄. 원본: `APDiffLine`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

impl DiffLine {
    pub fn new(kind: DiffKind, text: impl Into<String>) -> Self {
        Self { kind, text: text.into() }
    }
}

/// 더한 줄 수와 뺀 줄 수. 원본: `apDiffCount` 가 돌려주는 `(add:, del:)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffCount {
    pub add: usize,
    pub del: usize,
}

// 두 글의 곱이 이보다 크면 줄 견주기를 하지 않는다(보드가 멈추지 않게)
const MAX_DIFF_CELLS: usize = 4_000_000;
// 지시 길이 상한(엔진이 지시를 시스템 프롬프트 인자로 넘긴다)
const MAX_INSTRUCTION_CHARS: usize = 100_000;

// 엔진이 지시를 읽을 때처럼 앞뒤 빈 줄·빈칸을 자르고 줄로 나눈다. 줄 끝의 `\r` 하나는 뗀다
fn split_lines(s: &str) -> Vec<&str> {
    let t = s.trim();
    if t.is_empty() {
        return Vec::new();
    }
    t.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// 줄 차이 — 엔진이 지시를 읽을 때처럼 앞뒤 빈 줄·빈칸을 자른 두 글을 줄로 견준다(가장 긴 공통 줄, 뺀 줄을 더한 줄보다 앞에).
/// 줄이 너무 많으면(둘의 곱이 400만 넘음) 모두 빼고 모두 더한 것으로 본다(보드가 멈추지 않게). 원본: `apLineDiff`
pub fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a = split_lines(old);
    let b = split_lines(new);
    if a.len().saturating_mul(b.len()) > MAX_DIFF_CELLS {
        return a
            .iter()
            .map(|l| DiffLine::new(DiffKind::Del, *l))
            .chain(b.iter().map(|l| DiffLine::new(DiffKind::Add, *l)))
            .collect();
    }
    // Swift 의 문자열 같음은 정규 등가라서 NFC 로 견준다
    let ka: Vec<String> = a.iter().map(|l| l.nfc().collect()).collect();
    let kb: Vec<String> = b.iter().map(|l| l.nfc().collect()).collect();
    let w = b.len() + 1;
    // t[i * w + j] = a[i..] 와 b[j..] 의 가장 긴 공통 줄 수
    let mut t = vec![0u32; (a.len() + 1) * w];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i * w + j] =
                if ka[i] == kb[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
        }
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && ka[i] == kb[j] {
            out.push(DiffLine::new(DiffKind::Same, a[i]));
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || t[(i + 1) * w + j] >= t[i * w + j + 1]) {
            out.push(DiffLine::new(DiffKind::Del, a[i]));
            i += 1;
        } else {
            out.push(DiffLine::new(DiffKind::Add, b[j]));
            j += 1;
        }
    }
    out
}

/// 더한 줄·뺀 줄 수. 원본: `apDiffCount`
pub fn diff_count(d: &[DiffLine]) -> DiffCount {
    DiffCount {
        add: d.iter().filter(|l| l.kind == DiffKind::Add).count(),
        del: d.iter().filter(|l| l.kind == DiffKind::Del).count(),
    }
}

/// 지시 머리 줄 — "지난 판(10월 20일)과 차이 +2 −1" / "지난 판(…)과 같음" / "지난 판 없음 …"(날짜 글자는 보드가 만든다).
/// 빼기 기호는 `−`(U+2212)다. 원본: `apInstructionHead`
pub fn instruction_head(old: Option<&str>, new: Option<&str>, date: &str) -> String {
    let Some(o) = old else {
        return if new.is_none() {
            "지시 없음 · 공통 규칙만으로 돕니다".to_string()
        } else {
            "지난 판 없음 · 이 프로젝트에서 처음 쓰는 지시".to_string()
        };
    };
    let Some(n) = new else {
        return format!("지난 판({date})이 있으나 지금 세트에 지시가 없습니다");
    };
    let c = diff_count(&line_diff(o, n));
    if c.add + c.del == 0 {
        format!("지난 판({date})과 같음")
    } else {
        format!("지난 판({date})과 차이 +{} \u{2212}{}", c.add, c.del)
    }
}

// Swift `String.count` 에 가깝게: 글자 수에서 `\r\n` 한 쌍을 한 글자로 센다
fn char_count(s: &str) -> usize {
    s.chars().count() - s.matches("\r\n").count()
}

/// 고친 지시를 저장해도 되는가 — 빈 글은 저장하지 않는다(지시 없이 돌리는 것은 세트 파일을 지우는 일이라 이 자리에서 하지 않는다).
/// 엔진이 지시를 시스템 프롬프트 인자로 넘기므로 길이 상한(10만 자). 되면 `None`. 원본: `apInstructionSaveProblem`
pub fn instruction_save_problem(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return Some("빈 지시는 저장하지 않습니다. 고친 글을 넣어 주세요".to_string());
    }
    let n = char_count(t);
    if n > MAX_INSTRUCTION_CHARS {
        return Some(format!("지시가 너무 깁니다({n}자, 10만 자까지)"));
    }
    None
}
