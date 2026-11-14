//! 자율주행 규칙 — 하네스 읽기. 원본 `Sources/AutopilotRules/Harness.swift` 를 1:1 로 옮긴다.
//!
//! 저장소 하네스(CLAUDE.md·.claude 등)에서 사람 확인·승인·묻기·미리보기·기다리기·바깥 쓰기·hook 막기 자리
//! (확인 지점)를 기계로 찾고, 자율주행 지시가 그 자리를 이름(파일:줄)으로 다뤘는지 보고, 빠진 자리는
//! 출발 카드로 묻는다. 하네스 지문(파일마다 FNV-1a 64)을 지난 세트 기준과 견주어 바뀐 파일만 세트
//! 이어 쓰기에 넘긴다. 하네스 파일 읽기는 포트([`HarnessSource`])로만 한다.
//!
//! 이식 선언(원본 순서):
//! - `APHarnessPoint` → [`HarnessPoint`] · `apHarnessKindLabels` → [`harness_kind_label`]
//! - 파일 판정(비공개 상수 + `apHarnessFileWanted`) → `HARNESS_NAMES`·`HARNESS_DEPTH`·`CLAUDE_SKIP`·
//!   `TEXT_EXTS`·[`HARNESS_WALK_SKIP`]·[`piece_ext`]·[`harness_file_wanted`]
//! - 줄 꼴(`person`·`shapes`·`negAfter`·`negBefore`·`negRest`·`clause`·`hits`·`lineKind`) → 같은 이름(snake_case, 비공개)
//! - `apHarnessPointMax` → [`HARNESS_POINT_MAX`] · `scopes`·`apHarnessPoints` → [`harness_points`]
//! - `covers`·`placeTail`·`apHarnessMissing` → `fold_full`·`place_tail`·[`harness_missing`]
//! - `apInstructionForbiddenLines`·`apInstructionClean` → [`instruction_forbidden_lines`], [`instruction_clean`]
//! - `apHarnessCard`·`apHarnessCardId` → [`harness_card`], [`harness_card_id`]
//! - `apSeedScenarioPathOK`·`apUnitDirOK`·`apPinnedScenarioPath` → [`seed_scenario_path_ok`], [`unit_dir_ok`], [`pinned_scenario_path`]
//! - 하네스 정책(`apHarnessPolicy*`) → [`harness_policy_class`] 외 `harness_policy_*`
//! - 모으기(`APWalkStep`·`APHarnessSource`·`apHarnessCollect`·`apHarnessTexts`) → [`WalkStep`], [`HarnessSource`], [`harness_collect`], [`harness_texts`]
//! - 기준(`apHarnessChanges`) → [`harness_changes`] · 기준 파일(`apHarnessBaseline*`) → [`harness_baseline_parse`] 외
//! - 상태(`apHarnessState`·`apHarnessStateNote`) → [`harness_state`], [`harness_state_note`]
//!
//! 원본과의 차이:
//! - NSRegularExpression 대신 이 파일 안 비공개 정규식 해석기를 쓴다(`regex` 의존성 없음). 원본 꼴이 쓰는
//!   범위만 지원한다(겹침 2 겹, `[...]` 범위·부정, `? * + {m,n}` 게으른 `?` 포함, `\b \s \S \n`, `^ $`).
//!   대소문자 접기는 ASCII 만이라 원본(유니코드 접기)과 다를 수 있으나 쓰인 꼴엔 영향이 없다.
//! - `placeTail`·인용 앞뒤 검사·`apUnitDirOK` 는 꼴이 고정적이라 정규식 해석기 없이 손으로 검사한다.
//! - 글자 수(`String.count`·`prefix(n)`)는 [`crate::rules::formats::grapheme_len`] 기준, `fold_full` 은
//!   전체 문자열 NFC 합치기 + 소문자(path_key 조각 접기와 다르게 전체에 한 번).
//! - `apFNV64`(Import.swift, 미이식) 계산은 문자열 utf8 바이트마다 XOR 하는 것이 원본 `apFNV64Data` 와 같아
//!   아래 `fnv64_data` 하나로 둔다. Import 이식 후에는 그쪽 함수로 바꾼다(돌려 쓰기).
//! - `apForbiddenLines`(Draft.swift)는 비공개였던 `forbidden_lines` 로 여기 둔다(notify_rules 가
//!   redact 를 둔 방식과 같다). Draft 이식이 끝났지만 이 월줄 판정(`instruction_clean`)도 같은 꼴을 쓰고
//!   줄 꼴의 정규식 상자(`Pattern`)가 이 파일 안에 있어 그대로 두고 공개해 `rules::draft` 가 돌려 쓴다.
//! - `APHarnessSource` 포트도 ports.rs 통합 전까지 이 파일에 둔다(notify_rules 의 NoticeClock 처럼).

use crate::rules::formats::grapheme_len;
use crate::rules::path_key::path_key;
use crate::rules::scenarios::{scenario_path_ok, CardOption, CardText};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

// MARK: - 하네스 확인 지점

/// 저장소 하네스에서 기계로 찾은 확인 지점 하나 — 묻기·확인받기·승인·미리보기·기다리기·바깥 쓰기·hook 막기. 원본: `APHarnessPoint`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessPoint {
    /// 저장소 뿌리 기준 상대 경로
    pub file: String,
    /// 1부터
    pub line: i64,
    /// ask·confirm·approve·preview·wait·write·block
    pub kind: String,
    /// 그 줄(앞뒤 빈칸을 다듬고 200자까지)
    pub text: String,
}

impl HarnessPoint {
    /// 파일:줄 이름
    pub fn place(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }

    /// 줄을 다듬었을 때 앞뒤 빈칸을 접고 [`HARNESS_POINT_MAX`] 글자까지 자른다. 원본은 지점 만들기에서 같은 일을 한다.
    fn new(file: &str, line: i64, kind: &str, line_text: &str) -> Self {
        let text = cut_chars(line_text.trim(), HARNESS_POINT_MAX);
        HarnessPoint { file: file.to_string(), line, kind: kind.to_string(), text }
    }
}

/// 지점 종류의 사람 말 이름(모르는 종류는 그대로). 원본: `apHarnessKindLabels`
pub fn harness_kind_label(kind: &str) -> String {
    match kind {
        "ask" => "묻기",
        "confirm" => "확인받기",
        "approve" => "승인",
        "preview" => "미리보기",
        "wait" => "기다리기",
        "write" => "바깥 쓰기",
        "block" => "hook 막기",
        k => k,
    }
    .to_string()
}

// MARK: - 하네스 파일 판정

// 하네스 파일 이름(접은 꼴). 원본: `harnessNames`(비공개)
const HARNESS_NAMES: [&str; 3] = ["claude.md", "claude.local.md", ".mcp.json"];
// 폴더 깊이 상한. 원본: `harnessDepth`(비공개)
const HARNESS_DEPTH: usize = 4;
// .claude/ 안에서 내려가지 않는 자리(git 작업 트리·claude 임시 자리). 원본: `claudeSkip`(비공개)
const CLAUDE_SKIP: [&str; 2] = ["worktrees", ".cc-writes"];
// .claude/ 안에서 읽는 글 파일 꼴(확장자 없는 hook 스크립트도 읽는다). 원본: `textExts`(비공개)
const TEXT_EXTS: [&str; 18] = [
    "md", "markdown", "txt", "json", "jsonc", "yaml", "yml", "toml", "sh", "bash", "zsh", "py", "js",
    "mjs", "cjs", "ts", "rb", "pl",
];
// 지점 줄글 최대(글자 수). 원본: `apHarnessPointMax`
pub const HARNESS_POINT_MAX: usize = 200;
// 저장소에서 내려가지 않는 폴더. 원본: `apHarnessWalkSkip`
pub const HARNESS_WALK_SKIP: [&str; 12] = [
    ".git", "node_modules", "build", "dist", ".gradle", "target", ".venv", ".build", "out", ".idea",
    "deriveddata", "pods",
];

// 마지막 조각의 확장자(없으면 빈 글). Swift `split(separator:)` 처럼 빈 조각은 버린다.
fn piece_ext(last: &str) -> String {
    if !last.contains('.') {
        return String::new();
    }
    last.split('.').rev().find(|s| !s.is_empty()).unwrap_or("").to_string()
}

// 묶음(확장 자소) 덩어리 검사 — formats.rs 의 first_cluster_len·cut_after 와 같은 알고리즘인데 그쪽 파일이
// PORTED.json 지문에 묶여 고칠 수 없어 여기 둔다(scenarios.rs 의 prefix_graphemes 도 같은 전례).
fn cluster_len(s: &str) -> usize {
    let mut it = s.char_indices();
    let Some((_, first)) = it.next() else { return 0 };
    let (mut prev, mut ri) = (first, usize::from(ri_char(first)));
    let (mut pict_open, mut zwj_pending) = (pict_char(first), false);
    for (i, c) in it {
        let (p, q) = (hangul_class(prev), hangul_class(c));
        let free = !hs_control(prev) && !hs_control(c); // 제어 글자 앞뒤는 늘 끊긴다(CR LF 만 예외)
        let join = (prev == '\r' && c == '\n')
            || (free
                && (extend_char(c)
                    || (prev == '\u{200D}' && zwj_pending && pict_char(c))
                    || (ri_char(prev) && ri_char(c) && ri % 2 == 1)
                    || (p == 1 && matches!(q, 1 | 2 | 4 | 5))
                    || (matches!(p, 4 | 2) && matches!(q, 2 | 3))
                    || (matches!(p, 5 | 3) && q == 3)));
        if !join {
            return i;
        }
        zwj_pending = if c == '\u{200D}' { pict_open } else { zwj_pending && extend_char(c) };
        if !extend_char(c) && !(prev == '\u{200D}' && pict_char(c)) {
            pict_open = pict_char(c);
        }
        ri = if ri_char(c) { ri + 1 } else { 0 };
        prev = c;
    }
    s.len()
}

fn hs_control(c: char) -> bool {
    let v = c as u32;
    v < 0x20 || (0x7F..=0x9F).contains(&v) || v == 0x2028 || v == 0x2029
}

fn extend_char(c: char) -> bool {
    let v = c as u32;
    unicode_normalization::char::is_combining_mark(c) || v == 0x200C || v == 0x200D
        || (0x1F3FB..=0x1F3FF).contains(&v) || (0xE0020..=0xE007F).contains(&v)
}

fn pict_char(c: char) -> bool {
    let v = c as u32;
    (0x1F000..=0x1FAFF).contains(&v)
        || (0x2600..=0x27BF).contains(&v)
        || (0x2300..=0x23FF).contains(&v)
        || (0x2B00..=0x2BFF).contains(&v)
        || matches!(v, 0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139)
}

fn ri_char(c: char) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&(c as u32))
}

// 1 L, 2 V, 3 T, 4 LV, 5 LVT, 0 그 밖
fn hangul_class(c: char) -> u8 {
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

// trim 을 한 뒤 n 묶음 글자까지 자른다(Swift `prefix(n)` 와 같은 묶음 기준).
pub(crate) fn cut_chars(s: &str, n: usize) -> String {
    let (mut at, mut kept) = (0, 0);
    let mut out = String::new();
    while at < s.len() && kept < n {
        let len = cluster_len(&s[at..]);
        out.push_str(&s[at..at + len]);
        at += len;
        kept += 1;
    }
    out
}

/// 읽을 하네스 파일인가(저장소 뿌리 기준 상대 경로, 대소문자·유니코드 정규화를 접어 봄). 원본: `apHarnessFileWanted`
pub fn harness_file_wanted(rel: &str) -> bool {
    let raw: Vec<&str> = rel.split('/').collect();
    // Swift `split(separator:omittingEmptySubsequences:false)` 와 같아 빈 조각을 버리지 않아 그대로 남는다.
    if raw.iter().any(|p| *p == ".." || *p == "." || p.is_empty()) {
        return false;
    }
    let folded = path_key(rel);
    let parts: Vec<&str> = folded.split('/').collect();
    let Some(last) = parts.last().copied() else { return false };
    if let Some(ci) = parts.iter().position(|p| *p == ".claude") {
        if !(ci <= HARNESS_DEPTH && parts.len() > ci + 1) {
            return false;
        }
        if parts[..ci].iter().any(|p| HARNESS_WALK_SKIP.contains(p)) {
            return false;
        }
        if CLAUDE_SKIP.contains(&parts[ci + 1]) {
            return false;
        }
        let ext = piece_ext(last);
        return ext.is_empty() || TEXT_EXTS.contains(&ext.as_str());
    }
    if !HARNESS_NAMES.contains(&last) || parts.len() > HARNESS_DEPTH + 1 {
        return false;
    }
    !parts[..parts.len() - 1].iter().any(|p| HARNESS_WALK_SKIP.contains(p))
}

// MARK: - 작은 정규식 해석기(비공개)

// NSRegularExpression 이 쓰인 원본 꼴만 지원한다: 겹침 `(…)` 두 겹, `[...]`(범위·부정),
// `? * + {m,n}`(되풀이 뒤의 `?` 는 게으름), `\b \s \S \n \. \\` 이어쓰기, `. ^ $`.
// 대소문자 접기는 ASCII 만 하고, 글자 위치는 유니코드 글자(char) 위치를 쓴다(원본은 UTF-16 코드 단위
// 위치지만 위치 견주기만 하므로 이번 규칙엔 영향이 없다).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pat {
    /// 한 글자 고정(컴파일에 대소문자를 접었는지와 함께: 접었으면 견주기에도 접는다)
    Ch(char, bool),
    /// 글자 집합(쪽 목록, 뒤집음, 대소문자 접기)
    Set(Vec<ClassItem>, bool, bool),
    /// 가지: 겹친 잇기들
    Alt(Vec<Vec<Pat>>),
    /// 되풀이(본문, 최소, 최대, 게으름)
    Rep(Box<Pat>, usize, usize, bool),
    /// 줄 처음(`^`)
    Bos,
    /// 줄 끝(`$`)
    Eos,
    /// 낱말 경계(`\b`)
    WB,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassItem {
    One(char),
    Range(char, char),
    /// `\s` — 탭·줄바꿈·폼피드·CR 및 공간글자
    SpaceW,
    /// `\S`
    NotSpaceW,
    /// `\n`
    NewLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    nodes: Vec<Pat>,
}

impl Pattern {
    // 꼴에 남은 조각이 있으면 panic — Swift `try!` 의 거울
    fn compile(pat: &str, ci: bool) -> Pattern {
        let chars: Vec<char> = pat.chars().collect();
        let mut p = PatParser { cs: &chars, at: 0 };
        let nodes = p.alt_nodes(ci, 0);
        if p.at != chars.len() {
            panic!("정규식 꼴을 다 읽지 못했다({} 자에서 남음): {}", p.at, pat);
        }
        Pattern { nodes }
    }

    /// 처음 맞은 (시작, 끝) 자리 — NSRegularExpression `firstMatch` 와 같은 왼쪽 먼저
    fn find(&self, txt: &[char], from: usize) -> Option<(usize, usize)> {
        for start in from..=txt.len() {
            if let Some(end) = self.first_end(txt, start) {
                return Some((start, end));
            }
        }
        None
    }

    /// 이어지는 맞음 전부(겹치지 않게, 왼쪽 먼저). 원본 `matches(in:)` 와 같은 차례다.
    fn find_all(&self, txt: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at <= txt.len() {
            let Some((s, e)) = self.find(txt, at) else { break };
            out.push((s, e));
            at = if e == s { e + 1 } else { e };
        }
        out
    }

    /// 맞는 곳이 있는가
    fn hits(&self, txt: &[char]) -> bool {
        self.find(txt, 0).is_some()
    }

    // 한 시작 자리에서 가장 먼저 맞아 떨어지는 끝 자리
    fn first_end(&self, txt: &[char], at: usize) -> Option<usize> {
        let mut end = None;
        self.matches_from(txt, at, &mut |e| {
            end = Some(e);
            true
        });
        end
    }

    fn matches_from(&self, txt: &[char], start: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
        m_seq(&self.nodes, txt, start, k)
    }
}

// 꼴 해석기 — 가지(|) → 잇기 → 꾸러미 차례로 내려간다
struct PatParser<'a> {
    cs: &'a [char],
    at: usize,
}

impl<'a> PatParser<'a> {
    fn peek(&self) -> Option<char> {
        self.cs.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.at += 1;
        }
        c
    }

    fn shown(&self) -> String {
        self.cs.iter().collect()
    }

    fn alt_nodes(&mut self, ci: bool, depth: usize) -> Vec<Pat> {
        let mut branches = vec![self.seq(ci, depth)];
        while self.peek() == Some('|') {
            self.bump();
            branches.push(self.seq(ci, depth));
        }
        if branches.len() == 1 {
            return branches.pop().unwrap_or_default();
        }
        vec![Pat::Alt(branches)]
    }

    // 그룹 안의 가지들(각각 잇기)
    fn alt_branches(&mut self, ci: bool, depth: usize) -> Vec<Vec<Pat>> {
        let mut branches = vec![self.seq(ci, depth)];
        while self.peek() == Some('|') {
            self.bump();
            branches.push(self.seq(ci, depth));
        }
        branches
    }

    fn seq(&mut self, ci: bool, depth: usize) -> Vec<Pat> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            if out.is_empty() && matches!(c, '?' | '*' | '+' | '{') {
                panic!("꼬리가 자리 없이 나왔다({})", c);
            }
            let atom = self.atom(ci, depth);
            out.push(self.quantified(atom));
        }
        out
    }

    // 자리 하나. 자리 뒤의 되풀이 표는 seq 에서 붙인다.
    fn atom(&mut self, ci: bool, depth: usize) -> Pat {
        let c = self.bump().unwrap_or_else(|| panic!("정규식 꼴이 부족하다: {}", self.shown()));
        match c {
            '(' => {
                if depth >= 2 {
                    panic!("정규식 겹침은 두 겹까지: {}", self.shown());
                }
                let branches = self.alt_branches(ci, depth + 1);
                if self.bump() != Some(')') {
                    panic!("정규식 닫는 괄호 없음");
                }
                Pat::Alt(branches)
            }
            ')' | '|' => panic!("꼴 해석기가 잘못 들어갔다({})", c),
            '[' => self.class(ci),
            '.' => Pat::Set(vec![ClassItem::One('\n')], true, false),
            '^' => Pat::Bos,
            '$' => Pat::Eos,
            '\\' => {
                let e = self.bump().unwrap_or_else(|| panic!("어긋난 이어쓰기"));
                match e {
                    'b' => Pat::WB,
                    'n' => Pat::Set(vec![ClassItem::One('\n')], false, false),
                    's' => Pat::Set(vec![ClassItem::SpaceW], false, false),
                    'S' => Pat::Set(vec![ClassItem::SpaceW], true, false),
                    't' => Pat::Set(vec![ClassItem::One('\t')], false, false),
                    _ => Pat::Ch(self.fold(e, ci), ci),
                }
            }
            _ => Pat::Ch(self.fold(c, ci), ci),
        }
    }

    // 자리 뒤의 되풀이 꼬리(? * + {m,n} 을 잇달아 묶고, 그 뒤의 ? 는 게으름)
    fn quantified(&mut self, p: Pat) -> Pat {
        let mut cur = p;
        loop {
            let (lo, hi) = match self.peek() {
                Some('?') => {
                    self.bump();
                    (0, 1)
                }
                Some('*') => {
                    self.bump();
                    (0, usize::MAX)
                }
                Some('+') => {
                    self.bump();
                    (1, usize::MAX)
                }
                Some('{') => {
                    self.bump();
                    self.counted()
                }
                _ => return cur,
            };
            let lazy = if self.peek() == Some('?') {
                self.bump();
                true
            } else {
                false
            };
            cur = Pat::Rep(Box::new(cur), lo, hi, lazy);
        }
    }

    // {m,n} 꼴 읽기({m} 은 {m,m} 로)
    fn counted(&mut self) -> (usize, usize) {
        let m = self.digits().unwrap_or_else(|| panic!("되풀이 수 없음"));
        if self.peek() == Some('}') {
            self.bump();
            return (m, m);
        }
        if self.bump() != Some(',') {
            panic!("되풀이 꼴 {} 에 쉼표 없음", m);
        }
        let n = self.digits().unwrap_or(usize::MAX);
        if self.bump() != Some('}') {
            panic!("되풀이 꼴 닫는 갈고리 없음");
        }
        if n < m {
            panic!("되풀이 뒷수가 앞수보다 작다");
        }
        (m, n)
    }

    fn digits(&mut self) -> Option<usize> {
        let mut v: usize = 0;
        let mut any = false;
        while let Some(c) = self.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            v = v.saturating_mul(10).saturating_add((c as u8 - b'0') as usize);
            any = true;
            self.bump();
        }
        any.then_some(v)
    }

    // […] — 첫 ^ 는 뒤집음, 범위(a-b), \n \s \S 이어쓰기, 끝의 - 는 낱글자
    fn class(&mut self, ci: bool) -> Pat {
        let mut neg = false;
        if self.peek() == Some('^') {
            self.bump();
            neg = true;
        }
        let mut items = Vec::new();
        loop {
            let c = self.bump().unwrap_or_else(|| panic!("정규식 닫는 대괄호 없음: {}", self.shown()));
            if c == ']' && !items.is_empty() {
                break;
            }
            let lo = if c == '\\' {
                let e = self.bump().unwrap_or_else(|| panic!("어긋난 이어쓰기"));
                match e {
                    'n' => {
                        items.push(ClassItem::NewLine);
                        continue;
                    }
                    's' => {
                        items.push(ClassItem::SpaceW);
                        continue;
                    }
                    'S' => {
                        items.push(ClassItem::NotSpaceW);
                        continue;
                    }
                    _ => e,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.cs.get(self.at + 1).copied() != Some(']') {
                self.bump(); // -
                let hi = self.bump().unwrap_or_else(|| panic!("범위 끝 글자 없음"));
                let hi = if hi == '\\' {
                    self.bump().unwrap_or_else(|| panic!("범위 끝 이어쓰기 없음"))
                } else {
                    hi
                };
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::One(lo));
            }
        }
        Pat::Set(items, neg, ci)
    }

    fn fold(&self, c: char, ci: bool) -> char {
        if ci {
            c.to_ascii_lowercase()
        } else {
            c
        }
    }
}

// MARK: - 꼴 맞추기(계속 넘겨 짚기)

fn m_seq(nodes: &[Pat], txt: &[char], at: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    match nodes.split_first() {
        None => k(at),
        Some((first, rest)) => pat_matched(first, txt, at, &mut |a| m_seq(rest, txt, a, k)),
    }
}

fn pat_matched(p: &Pat, txt: &[char], at: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    match p {
        Pat::Ch(c, folded) => match txt.get(at) {
            // 접었으면 낱글자도 접어 견주고(대소문자 무시), 아니면 그대로 견준다(대소문자 구별)
            Some(&x) if if *folded { x.to_ascii_lowercase() == *c } else { x == *c } => k(at + 1),
            _ => false,
        },
        Pat::Set(items, neg, ci) => match txt.get(at) {
            Some(&x) => {
                let fold = |c: char| if *ci { c.to_ascii_lowercase() } else { c };
                let x_in = items.iter().any(|it| match it {
                    ClassItem::One(o) => fold(x) == fold(*o),
                    ClassItem::Range(a, b) => {
                        let (x, a, b) = (fold(x), fold(*a), fold(*b));
                        a <= x && x <= b
                    }
                    ClassItem::SpaceW => regex_space(x),
                    ClassItem::NotSpaceW => !regex_space(x),
                    ClassItem::NewLine => x == '\n',
                });
                if x_in != *neg {
                    k(at + 1)
                } else {
                    false
                }
            }
            None => false,
        },
        Pat::Alt(branches) => {
            for b in branches {
                if m_seq(b, txt, at, k) {
                    return true;
                }
            }
            false
        }
        Pat::Rep(body, lo, hi, lazy) => rep_matched(body, txt, at, 0, *lo, *hi, *lazy, k),
        Pat::Bos => at == 0 && k(at),
        Pat::Eos => at == txt.len() && k(at),
        Pat::WB => {
            let before = at > 0 && is_word_char(txt[at - 1]);
            let after = at < txt.len() && is_word_char(txt[at]);
            before != after && k(at)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rep_matched(
    body: &Pat,
    txt: &[char],
    at: usize,
    so_far: usize,
    lo: usize,
    hi: usize,
    lazy: bool,
    k: &mut dyn FnMut(usize) -> bool,
) -> bool {
    let more = |k: &mut dyn FnMut(usize) -> bool| {
        so_far < hi
            && pat_matched(body, txt, at, &mut |a| {
                // 본문이 빈 자리에도 맞으면 끝 없이 도니 그만한다(이 규칙의 꼴엔 빈 본문이 없다).
                a != at && rep_matched(body, txt, a, so_far + 1, lo, hi, lazy, k)
            })
    };
    if lazy {
        if so_far >= lo && k(at) {
            return true;
        }
        return more(k);
    }
    if more(k) {
        return true;
    }
    so_far >= lo && k(at)
}

// ICU `\s` — 탭·줄바꿈·폼피드·CR 및 공간글자(Zs·Zl·Zp)
fn regex_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{C}' | '\r' | '\u{2028}' | '\u{2029}')
        || matches!(
            c,
            ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        )
}

// ICU 낱말 글자 — 알파벳·숫자·밑줄(유니코드 글자 포함)
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// MARK: - 줄 꼴

// 한 줄에 처음 맞은 종류 하나(바깥 쓰기 → hook 막기 → 묻기 → 확인 → 승인 → 미리보기 → 기다리기 차례).
// tool: 도구·명령 이름(울타리 코드 안에서도 봄), prose: 사람 말 꼴(울타리 코드·JSON 밖에서만), hook: 설정·hook 스크립트에서만
struct Shape {
    kind: &'static str,
    re: Pattern,
    scope: &'static str,
}

// 원본은 모든 줄 꼴을 대소문자 접어 봄으로 만든다.
fn shape(kind: &'static str, scope: &'static str, p: &str) -> Shape {
    Shape { kind, re: Pattern::compile(p, true), scope }
}

// 사람이 걸린 자리의 사람 말(꼴 안의 $PERSON 자리에 들어간다)
const PERSON: &str = "(사람|사용자|유저|담당자|리뷰어|팀장|관리자|요청자|운영자|기획자|PM|리드)";

// 꼴 안의 $PERSON 자리에 사람 낱말을 넣는다
fn shape_str(p: &str) -> String {
    p.replace("$PERSON", PERSON)
}

fn harness_shapes() -> &'static [Shape] {
    static S: OnceLock<Vec<Shape>> = OnceLock::new();
    S.get_or_init(|| {
        vec![
            // 바깥 쓰기 도구·명령(설계 2장: Jira·Confluence 쓰기 도구 이름 등). 읽기 도구(getJiraIssue 등)는 잡지 않는다
            shape("write", "tool", "createJiraIssue|editJiraIssue|addCommentToJiraIssue|transitionJiraIssue|createConfluence(Page|Content|FooterComment|InlineComment)|updateConfluence(Page|Content)|mcp__[A-Za-z0-9_.-]+__[A-Za-z0-9_-]*(create|update|edit|add|delete|remove|post|send|publish|transition|comment|write|upload|archive|merge|assign)|\\bgit\\s+(-C\\s+\\S+\\s+)?push\\b|\\bgh\\s+(pr|issue|release)\\s+(create|merge|comment|edit|close)\\b|\\bglab\\s+mr\\s+(create|merge)\\b"),
            shape("write", "prose", r"(jira|지라|confluence|컨플루언스|컨플|notion|노션)[^.。!?\n]{0,20}?(만들|만든|생성|올리|올린|댓글|코멘트|게시|업데이트|갱신|등록|전환|바꾸|바꾼)|(slack|슬랙|메신저|이메일|메일|webhook|웹훅)[^.。!?\n]{0,12}?(알리|알린|보내|보낸|올리|올린|공유|게시)"),
            // hook 막기·권한 묻기(설정·hook 스크립트)
            shape("block", "hook", r#"\bexit\s+2\b|sys\.exit\(\s*2\s*\)|process\.exit\(\s*2\s*\)|"decision"\s*:\s*"block"|permissionDecision"?\s*[:=]\s*"?(deny|ask)\b|"continue"\s*:\s*false|"(PreToolUse|PermissionRequest|UserPromptSubmit)"\s*:"#),
            shape("ask", "hook", r#""ask"\s*:\s*\["#),
            shape("ask", "tool", "AskUserQuestion"),
            // 묻기
            shape("ask", "prose", r"묻(는다|습니다|고 |고서|기 전|어 보|어보|어야)|물어(보|본|봐)|여쭤|되물|질문(하고|한 뒤|한 후|해서|해야|한다|합니다|을 하|을 남기고)|\bask (the )?(user|human|me|for (clarification|input|permission))\b|\bask before\b|clarifying question"),
            // 확인받기
            shape("confirm", "prose", &shape_str(r"확인(을)? ?받|확인(을)? ?(요청|구하|거치|거친|거쳐)|컨펌|검토(를)? ?받|리뷰(를)? ?받|허락(을)? ?(받|구)|동의(를)? ?(받|구)|(OK|오케이)(를|을)? ?받|$PERSON(에게|한테|께|의)[^.。!?\n]{0,15}?확인|confirm (with|before)|ask for confirmation|get (explicit )?confirmation|human review|sign[- ]?off")),
            // 승인("결제 승인 API" 같은 이름 꼴은 잡지 않는다: 받다·요청·거침·전후·없이·대기만)
            shape("approve", "prose", r"승인(을|이)? ?(받|요청|구하|거치|거친|거쳐|필요|난 뒤|된 뒤|후|뒤|전|없이|대기)|결재(를)? ?(받|올리)|approv(al|ed?)[^.\n]{0,20}?(from|by|before|required|needed)|(get|require|need|wait for|request)s? (an? )?approval"),
            // 미리보기(사람에게 보여 줌만. "미리보기 화면을 만든다" 같은 기능 이름은 잡지 않는다)
            shape("preview", "prose", r"미리 ?보(여|이)|미리보기(를|로)? ?(보여|보이|띄우|확인|공유)|미리보기로[^.。!?\n]{0,20}?(보여|보이|확인|공유)|show (a |the )?preview|preview (it |them |this )?(to|for|with) (the )?(user|human|reviewer)"),
            // 기다리기(사람·답이 걸린 자리만. "빌드가 끝날 때까지 기다린다" 는 잡지 않는다)
            shape("wait", "prose", &shape_str(r"$PERSON(의|이|가)? ?(답|대답|응답|회신|확인|승인|결정|지시|입력)[^.。!?\n]{0,12}?(기다|대기)|(답|대답|회신|승인|확인|검토|리뷰|지시|허락|결정)(을|를|이|가)? ?(기다리|기다린|기다려|기다릴|받을 때까지)|멈추고 ?(기다|대기)|멈춰 ?(서 ?)?기다|$PERSON[^.。!?\n]{0,10}?(기다리|기다린|기다려)|wait for (the )?(user|human|approval|confirmation|review|response|reply|input|sign[- ]?off)|pause (and|until) (the )?(user|human)")),
        ]
    })
}

// 맞은 꼴 바로 뒤(같은 마디 6자 안)의 부정 — "묻지 않고", "확인받지 않은", "기다리지 말고"
fn neg_after() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"^[^.。!?\n]{0,6}?(지 ?않|지 ?말|지 ?마|지 ?못|않는|않고|않아|않은|않습)", false))
}

// 맞은 꼴 바로 앞의 영어 부정 — "don't ask", "never wait for", "without approval"
fn neg_before() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"(don'?t|do not|never|no need to|without)\s+([a-z]+\s+){0,2}$", true))
}

// 나머지 마디의 부정(있으면 이중 부정이라 확인 지점으로 남긴다 — "승인 없이 합치지 않는다", "확인받지 않은 변경은 커밋하지 않는다")
fn neg_rest() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"않|말고|말아|금지|못 |안 된|안된|\bnot\b|\bnever\b|n't", true))
}

// 마디 끝 글자(문장 끝)
fn is_clause_end(c: char) -> bool {
    matches!(c, '.' | '。' | '!' | '?' | ';' | '\n')
}

// 처음 마디 끝 글자 앞까지만(Swift `rangeOfCharacter(from:)` 와 같은 첫 자리)
fn clause(s: &str) -> String {
    match s.find(|c: char| is_clause_end(c)) {
        Some(i) => s[..i].to_string(),
        None => s.to_string(),
    }
}

// 맞는 곳이 있는가(Swift `firstMatch(in:) != nil`)
fn hits(re: &Pattern, s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    re.hits(&chars)
}

// 이 줄에서 맞은 확인 지점 종류(없으면 None). scopes: 이 줄에 볼 꼴(tool·prose·hook)
fn line_kind(line: &str, scopes: &[&str]) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    for sh in harness_shapes() {
        if !scopes.contains(&sh.scope) {
            continue;
        }
        for (s, e) in sh.re.find_all(&chars) {
            // hook 막기·스크립트 안 도구 이름은 부정을 보지 않는다
            if sh.scope == "hook" || !scopes.contains(&"prose") {
                return Some(sh.kind.to_string());
            }
            let matched: String = chars[s..e].iter().collect();
            let after: String = chars[e..].iter().collect();
            let before_rev: String = chars[..s].iter().rev().collect();
            let before_text: String = clause(&before_rev).chars().rev().collect();
            let neg_a = hits(neg_after(), &after);
            let neg_b = hits(neg_before(), &before_text);
            let without = matched.ends_with("없이") || matched.to_ascii_lowercase().contains("without");
            if neg_a || neg_b || without {
                // 이중 부정이면 확인 지점: 부정 뒤 같은 마디에 다른 부정이 있음
                let mut rest = clause(&after);
                if neg_a {
                    let rest_chars: Vec<char> = rest.chars().collect();
                    if let Some((_, re)) = neg_after().find(&rest_chars, 0) {
                        rest = rest_chars[re..].iter().collect();
                    }
                }
                if hits(neg_rest(), &rest) {
                    return Some(sh.kind.to_string());
                }
                continue;
            }
            return Some(sh.kind.to_string());
        }
    }
    None
}

// MARK: - 확인 지점 채집

// 파일 꼴에 따라 볼 꼴 — 설정(json)은 도구 이름·hook·권한 묻기, hook 자리(.claude/hooks/ 또는 스크립트 확장자)는 도구 이름·hook 막기, 글 문서는 도구 이름·사람 말 꼴
fn scopes(rel: &str) -> Vec<&'static str> {
    let k = path_key(rel);
    let last = k.rsplit('/').next().unwrap_or(&k);
    let ext = match last.rsplit_once('.') {
        Some((_, e)) => e,
        None => "",
    };
    if ["md", "markdown", "txt"].contains(&ext) {
        vec!["tool", "prose"]
    } else if ["json", "jsonc"].contains(&ext) {
        // 설정의 허용·거부 목록에 든 도구 이름은 지시가 아니라 설정이다
        vec!["hook"]
    } else {
        // hook 스크립트·확장자 없는 이름·yaml 등
        vec!["tool", "hook"]
    }
}

// 견주기 접기 — 유니코드 정규화 뒤 아스키 소문자 접기(원본 `precomposedStringWithCanonicalMapping.lowercased()`)
fn fold_lax(s: &str) -> Vec<char> {
    s.nfc().collect::<String>().to_ascii_lowercase().chars().collect()
}

// 바늘 처음 맞음 자리
fn find_sub(s: &[char], needle: &[char], from: usize) -> Option<usize> {
    // 바늘이 풀보다 길면 원본 검색(range(of:))은 못 찾은 것으로 끝난다 — 범위를 풀보다 바늘을 자르고 뽑지 않는다
    if needle.is_empty() || needle.len() > s.len() {
        return None;
    }
    (from..=s.len() - needle.len()).find(|&i| s[i..i + needle.len()] == *needle)
}

// 이어진 숫자 뭉치 하나(자릿수 오버프은 원본 `Int(...)` 처럼 -1 로 두기)
fn span_digits(cs: &[char], at: usize) -> Option<(i64, usize)> {
    let d0 = at;
    let mut e = at;
    while e < cs.len() && cs[e].is_ascii_digit() {
        e += 1;
    }
    if e == d0 {
        return None;
    }
    let value: i64 = cs[d0..e].iter().collect::<String>().parse().unwrap_or(-1);
    Some((value, e))
}

// ":줄[범위][쉼표 목록]" 꼴을 자리 0 에서 읽어 걸친 줄 범위들을 되돌린다(원본 placeTail 을 손으로 옮김:
// ^:([0-9]+)(\s*[-~–]\s*([0-9]+))?((\s*,\s*[0-9]+(\s*[-~–]\s*[0-9]+)?)*) — 그룹 캡처 없이 같은 뜻)
fn place_tail(cs: &[char]) -> Option<Vec<(i64, i64)>> {
    if cs.first() != Some(&':') {
        return None;
    }
    let mut at = 1usize;
    let (a, a_end) = span_digits(cs, at)?;
    at = a_end;
    let mut ranges: Vec<(i64, i64)> = Vec::new();
    // 옵션 범위: \s*[-~–]\s*[0-9]+ — 뒤 숫자가 없으면 범위도 없던 것으로 백트랙
    let save = at;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    if at < cs.len() && matches!(cs[at], '-' | '~' | '\u{2013}') {
        at += 1;
        while at < cs.len() && regex_space(cs[at]) {
            at += 1;
        }
        match span_digits(cs, at) {
            Some((b, e)) => {
                at = e;
                ranges.push((a, b));
            }
            None => {
                at = save;
                ranges.push((a, a));
            }
        }
    } else {
        at = save;
        ranges.push((a, a));
    }
    // 옵션 목록: (\s*,\s*[0-9]+(\s*[-~–]\s*[0-9]+)?)*
    loop {
        while at < cs.len() && regex_space(cs[at]) {
            at += 1;
        }
        if at < cs.len() && cs[at] == ',' {
            at += 1;
            while at < cs.len() && regex_space(cs[at]) {
                at += 1;
            }
            if let Some((x, x_end)) = span_digits(cs, at) {
                at = x_end;
                let save2 = at;
                while at < cs.len() && regex_space(cs[at]) {
                    at += 1;
                }
                if at < cs.len() && matches!(cs[at], '-' | '~' | '\u{2013}') {
                    at += 1;
                    while at < cs.len() && regex_space(cs[at]) {
                        at += 1;
                    }
                    match span_digits(cs, at) {
                        Some((y, y_end)) => {
                            at = y_end;
                            ranges.push((x, y));
                        }
                        None => {
                            at = save2;
                            ranges.push((x, x));
                        }
                    }
                } else {
                    at = save2;
                    ranges.push((x, x));
                }
                continue;
            }
        }
        break;
    }
    Some(ranges)
}

// 지시 글이 그 확인 지점을 이름으로 다뤘나 — "파일:줄" 이 글에 있어야 한다(설계 판단: 이름으로 하나씩).
// 대소문자·유니코드 정규화를 접어 견주고, 앞이 경로 글자면(docs/CLAUDE.md 는 CLAUDE.md 가 아님) 뒤가 숫자로 이어지면(CLAUDE.md:30 은 :3 이 아님) 다룬 것이 아니다.
// 줄 범위(CLAUDE.md:3-5 · 3~5)와 쉼표 목록(CLAUDE.md:3,7)도 받는다
fn covers(text: &str, file: &str, line: i64) -> bool {
    let t = fold_lax(text);
    let f = fold_lax(file);
    if f.is_empty() {
        return false;
    }
    let mut from = 0usize;
    while let Some(r) = find_sub(&t, &f, from) {
        from = r + f.len();
        if r > 0 {
            let prev = t[r - 1];
            if prev.is_alphabetic() || prev.is_numeric() || "/._-".contains(prev) {
                continue;
            }
        }
        let tail: String = t[r + f.len()..].iter().collect();
        let tail_chars: Vec<char> = tail.chars().take(80).collect();
        let Some(ranges) = place_tail(&tail_chars) else { continue };
        if ranges.iter().any(|(a, b)| (*a).min(*b) <= line && line <= (*a).max(*b)) {
            return true;
        }
    }
    false
}

// 지시 글이 다루지 않은 확인 지점
pub fn harness_missing(points: &[HarnessPoint], instruction: &str) -> Vec<HarnessPoint> {
    points.iter().filter(|p| !covers(instruction, &p.file, p.line)).cloned().collect()
}

// 하네스 파일 글에서 확인 지점을 찾는다(줄마다 처음 맞은 종류 하나). 파일 차례(접은 경로)·줄 차례, 200곳까지. 울타리 코드(```) 안은 도구 이름만
pub fn harness_points(files: &[(String, String)]) -> Vec<HarnessPoint> {
    let mut out: Vec<HarnessPoint> = Vec::new();
    let mut fs: Vec<&(String, String)> = files.iter().collect();
    fs.sort_by_key(|a| path_key(&a.0));
    for f in fs {
        if !harness_file_wanted(&f.0) {
            continue;
        }
        let sc: Vec<&'static str> = scopes(&f.0);
        let prose_on = sc.contains(&"prose");
        let mut fence = false;
        for (i, raw) in f.1.split('\n').enumerate() {
            let line = raw.trim();
            if prose_on && (line.starts_with("```") || line.starts_with("~~~")) {
                fence = !fence;
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let view: Vec<&str> = if fence {
                sc.iter().copied().filter(|s| *s != "prose").collect()
            } else {
                sc.clone()
            };
            let Some(kind) = line_kind(line, &view) else { continue };
            out.push(HarnessPoint::new(&f.0, (i + 1) as i64, &kind, line));
            if out.len() >= HARNESS_POINT_MAX {
                return out;
            }
        }
    }
    out
}

// MARK: - 지시 글 금지 줄(Draft.swift 에서 옮김)

// 초안 지시의 금지 줄 — Draft.swift apForbiddenLines 를 옮긴다.
// (Draft 는 옮기지 않은 자리라 하네스 줄 판정에 함께 쓰이는 이웃 자리로 여기에 둔다; notify_rules redact 처럼 이웃 자리를 옮긴 예)
// 바깥 쓰기·되살림(update-ref)·push·하네스 지문을 시키는 꼴(대소문자 접음)
fn forbidden_shape() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"git\s+(-C\s+\S+\s+)?push|branch\s+-[fDd]\b|update-ref|createJiraIssue|editJiraIssue|addCommentToJiraIssue|transitionJiraIssue|create(Confluence)?Page|updateConfluence|mcp__", true))
}

// 하네스 지문 고치기 꼴(CLAUDE.md 는 대소문자 그대로: 대소문자 접지 않음)
fn harness_edit_shape() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"(CLAUDE\.md|\.claude/|\.mcp\.json)[^\n]{0,30}?(고치|고쳐|바꾸|바꿔|수정|편집|지우|지워|추가하|더하|더한)", false))
}

// 맞은 자리 뒤의 부정(이중 부정이면 금지 줄이 아니다) — 대소문자 접지 않음
fn forbidden_negation() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"^[^.!?;。]{0,16}?(않|말고|금지|하지 ?마|막혀|막힌|보류|없습니다|없다|못 )", false))
}

// 부정 창 — 맞은 자리 뒤 같은 마디 16자 안(원본 apNegationWindow)
pub const AP_NEGATION_WINDOW: usize = 16;

// 초안 지시의 금지 줄(맞는 줄 가운데 이중 부정 없는 줄, 앞뒤 공백을 뗀 160글자) —
// 하네스 지시 판정(instruction_clean)과 초안 검사(rules::draft)가 돌려 쓰게 공개한다.
pub fn forbidden_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .filter_map(|raw| {
            let line = raw.trim();
            let chars: Vec<char> = line.chars().collect();
            let mut all: Vec<(usize, usize)> = forbidden_shape().find_all(&chars);
            all.extend(harness_edit_shape().find_all(&chars));
            if all.is_empty() {
                return None;
            }
            // 부정이 맞은 꼴 바로 뒤 같은 마디 16자 안에 있으면 그 꼴은 받아 넘기고, 없어서(말해도 안 무시 못 하는 줄) 금지 줄. 원본 Draft.swift apForbiddenLines
            let bad = all.iter().any(|(_, e)| !forbidden_negation().hits(&chars[*e..]));
            if bad {
                Some(cut_chars(line, 160))
            } else {
                None
            }
        })
        .collect()
}

// MARK: - 하네스 지문 인용 판정

// 보류 고정 낱말(원본 apHarnessDeferPhrases)
const HARNESS_DEFER_PHRASES: [&str; 8] = [
    "보류해 next_start 에 남깁니다",
    "보류해 next_start 에 남긴다",
    "하지 않고 보류합니다",
    "하지 않고 보류한다",
    "보류합니다",
    "보류한다",
    "하지 않습니다",
    "하지 않는다",
];

// 인용 따옴표 짝(원본 quotePairs)
const QUOTE_PAIRS: [(&str, &str); 6] = [
    ("\"", "\""),
    ("\u{201C}", "\u{201D}"),
    ("「", "」"),
    ("『", "』"),
    ("'", "'"),
    ("\u{2018}", "\u{2019}"),
];

// 목록 머리(-* +·숫자.) 를 뗀 나머지(머리가 아니면 None). 원본 listHead 는 처음 자리에만 맞는다
fn without_list_head(line: &str) -> Option<String> {
    let cs: Vec<char> = line.chars().collect();
    let mut at = 0usize;
    match cs.first() {
        Some('-') | Some('*') | Some('+') => at += 1,
        Some(c) if c.is_ascii_digit() => {
            while at < cs.len() && cs[at].is_ascii_digit() {
                at += 1;
            }
            if at < cs.len() && (cs[at] == '.' || cs[at] == ')') {
                at += 1;
            } else {
                return None;
            }
        }
        _ => return None,
    }
    // \s+
    if at >= cs.len() || !regex_space(cs[at]) {
        return None;
    }
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    Some(cs[at..].iter().collect::<String>().trim().to_string())
}

// [목록 머리]파일:줄[범위] 꼴을 자리 0 에서 읽어 (줄, 범위 끝) 을 되돌린다(원본 before 꼴을 손으로 옮김.
// ^\s*(([-*+]|[0-9]+[.)])\s+)?:([0-9]+)(\s*[-~–]\s*([0-9]+))?\s*$ — 대소문자 접음)
fn before_tail(b: &str, file: &str) -> Option<(i64, i64)> {
    let folded = b.nfc().collect::<String>().to_ascii_lowercase();
    let cs: Vec<char> = folded.chars().collect();
    let file_fold = fold_lax(file);
    let mut at = 0usize;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    // 옵션 목록 머리 ([-*+]|[0-9]+[.)])\s+ — 뒤에 \s+ 이 없으면 머리도 없던 것으로 백트랙
    let save = at;
    let head_span = match cs.get(at) {
        Some('-') | Some('*') | Some('+') => Some(at + 1),
        _ => {
            let d0 = at;
            while at < cs.len() && cs[at].is_ascii_digit() {
                at += 1;
            }
            if at > d0 && at < cs.len() && (cs[at] == '.' || cs[at] == ')') {
                Some(at + 1)
            } else {
                at = save;
                None
            }
        }
    };
    if let Some(mut e) = head_span {
        let d = e;
        while e < cs.len() && regex_space(cs[e]) {
            e += 1;
        }
        if e == d {
            at = save;
        } else {
            at = e;
        }
    }
    if cs.len() < at + file_fold.len() || cs[at..at + file_fold.len()] != file_fold[..] {
        return None;
    }
    at += file_fold.len();
    if cs.get(at) != Some(&':') {
        return None;
    }
    at += 1;
    let (lo, e) = span_digits(&cs, at)?;
    at = e;
    // 옵션 범위 \s*[-~–]\s*([0-9]+) — 뒤 숫자가 없으면 범위도 없던 것으로 백트랙
    let save = at;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    let high: i64;
    if at < cs.len() && matches!(cs[at], '-' | '~' | '\u{2013}') {
        at += 1;
        while at < cs.len() && regex_space(cs[at]) {
            at += 1;
        }
        match span_digits(&cs, at) {
            Some((h, e)) => {
                at = e;
                high = h;
            }
            None => {
                at = save;
                high = lo;
            }
        }
    } else {
        at = save;
        high = lo;
    }
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    if at != cs.len() {
        return None;
    }
    Some((lo, high))
}

// [보류 낱말][ (출발 )?카드 id][.] 꼴인가(원본 after 꼴을 손으로 옮김.
// ^\s*[:：]\s*(보류 낱말들)(\s*\((출발 )?카드 (카드 id|정책 id)\))?\s*[.。]?\s*$ — 대소문자 그대로)
fn after_tail(a: &str, p: &HarnessPoint) -> bool {
    let cs: Vec<char> = a.chars().collect();
    let starts = |cs: &[char], at: usize, words: &str| -> Option<usize> {
        let wc: Vec<char> = words.chars().collect();
        if cs.len() >= at + wc.len() && cs[at..at + wc.len()] == wc[..] {
            Some(at + wc.len())
        } else {
            None
        }
    };
    let mut at = 0usize;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    if cs.get(at) != Some(&':') && cs.get(at) != Some(&'：') {
        return false;
    }
    at += 1;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    let Some(mut at) = HARNESS_DEFER_PHRASES.iter().find_map(|w| starts(&cs, at, w)) else {
        return false;
    };
    // 옵션 카드 낱말: \s*\((출발 )?카드 (id)\)
    let save = at;
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    if cs.get(at) != Some(&'(') {
        at = save;
    } else {
        let depart = "출발 ";
        let card_head = "카드 ";
        let mut group_end: Option<usize> = None;
        for departed in [false, true] {
            let mut head = at + 1; // ( 를 넘김
            if departed {
                let Some(e) = starts(&cs, head, depart) else { continue };
                head = e;
            }
            let Some(e) = starts(&cs, head, card_head) else { continue };
            let base = e;
            for cand in [harness_card_id(p), harness_policy_id(p)] {
                let Some(e) = starts(&cs, base, &cand) else { continue };
                let mut end = e;
                while end < cs.len() && regex_space(cs[end]) {
                    end += 1;
                }
                if cs.get(end) == Some(&')') {
                    group_end = Some(end + 1);
                    break;
                }
            }
            if group_end.is_some() {
                break;
            }
        }
        match group_end {
            Some(e) => at = e,
            None => at = save,
        }
    }
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    if at < cs.len() && (cs[at] == '.' || cs[at] == '。') {
        at += 1;
    }
    while at < cs.len() && regex_space(cs[at]) {
        at += 1;
    }
    at == cs.len()
}

// 이 줄이 그 확인 지점을 인용한 하네스 줄인가 — [줄 머리]파일:줄[범위] "<그 지점 글>": 보류 낱말[(출발 카드 id)][.]
// 인용 앞·뒤 글은 금지 줄 검사를 한 번 더 한다(원본 quotedHarnessLine)
fn quoted_harness_line(line: &str, harness: &[HarnessPoint]) -> bool {
    for p in harness {
        // 인용 앞: 목록 머리 + 그 지점 파일 경로 그대로 + 그 줄(또는 그 줄을 덮는 범위). 인용 뒤: 정한 보류 글 + 그 지점의 실제 카드 id(선택)
        let mut texts: Vec<String> = vec![p.text.clone()];
        if let Some(rest) = without_list_head(&p.text) {
            texts.push(rest);
        }
        for q in texts {
            // 짧은 낱말은 사건 가운데 흔한 글자 얽힘으로 틀리게 인용되기 쉬워 뺀다(원본 q.count >= 6)
            if cluster_len(&q) < 6 {
                continue;
            }
            for (open, close) in QUOTE_PAIRS {
                let quoted = open.to_owned() + &q + close;
                let Some(r) = line.find(&quoted) else { continue };
                let before = &line[..r];
                let after = &line[r + quoted.len()..];
                let Some((lo, hi)) = before_tail(before, &p.file) else { continue };
                if !(lo.min(hi) <= p.line && p.line <= lo.max(hi)) {
                    continue;
                }
                if !after_tail(after, p) {
                    continue;
                }
                if !forbidden_lines(before).is_empty() || !forbidden_lines(after).is_empty() {
                    continue;
                }
                return true;
            }
        }
    }
    false
}

// 지시 글 다듬기 결과
pub struct InstructionClean {
    pub text: String,
    pub dropped: Vec<String>,
    pub forbidden: Vec<String>,
}

// 지시 글 다듬기(7단계 ⑫-7b): 금지 꼴이 든 줄 가운데 확인 지점(파일:줄)을 가리키는 줄은 지시에서 빼고(그 지점은 다루지 않은 자리가 되어 출발 카드로 묻는다),
// 확인 지점을 가리키지 않는 금지 줄은 forbidden 으로 돌려준다(준비를 멈춤). 정한 꼴의 인용 줄은 그대로 둔다.
// 하네스 원래 글을 옮겨 적으면 그 글의 낱말(예: "CLAUDE.md:12 \"스키마를 수정할 때는…\"" 의 수정)이 하네스 고치기 꼴에 걸리기 쉬워, 그 줄로 준비가 멈추지 않게 한다.
// 뺀 줄은 지시에 남지 않으므로 세션에 닿지 않는다(검사를 빠져나가는 길이 아니다)
pub fn instruction_clean(t: &str, harness: &[HarnessPoint]) -> InstructionClean {
    let mut keep: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    let mut forbidden: Vec<String> = Vec::new();
    for line in t.split('\n') {
        let bad = forbidden_lines(line);
        if bad.is_empty() || quoted_harness_line(line, harness) {
            keep.push(line.to_string());
            continue;
        }
        if harness.iter().any(|p| covers(line, &p.file, p.line)) {
            dropped.push(cut_chars(line.trim(), 160));
        } else {
            forbidden.extend(bad);
        }
    }
    InstructionClean { text: keep.join("\n"), dropped, forbidden }
}

// 인용한 하네스 줄 가운데 확인 지점을 가리키지 않는 인용도 금지 줄로 돌려준다(원본 apInstructionForbiddenLines)
pub fn instruction_forbidden_lines(t: &str, harness: &[HarnessPoint]) -> Vec<String> {
    instruction_clean(t, harness).forbidden
}

// MARK: - 출발 카드와 경로 검사

// 빠진 확인 지점의 출발 카드(막지 않고 묻는다, 2026-10-21 저녁 사용자 결정). 추천은 보류해 다음 출발로
pub const HARNESS_CARD_PREFIX: &str = "harness-";

// 빠진 확인 지점의 출발 카드(원본 apHarnessCard)
pub fn harness_card(p: &HarnessPoint) -> CardText {
    CardText {
        label: "하네스 확인 지점".to_string(),
        title: "이 확인 지점을 자율주행에서 어떻게 할까요: ".to_owned() + &p.place(),
        why: format!("{} (초안 세션이 쓴 자율주행 지시가 이 자리를 다루지 않았습니다.)", p.text),
        options: vec![
            CardOption {
                key: "defer".to_string(),
                label: "보류해 다음 출발로".to_string(),
                note: "이 자리에 닿으면 그 부분만 보류하고 next_start 에 남깁니다".to_string(),
            },
            CardOption {
                key: "follow".to_string(),
                label: "그대로 따름".to_string(),
                note: "하네스 글대로 합니다. 사람이 필요한 자리면 멈춰 보류합니다".to_string(),
            },
            CardOption {
                key: "drop".to_string(),
                label: "범위에서 뺌".to_string(),
                note: "이 자리가 걸린 일은 이번 실행에서 하지 않습니다".to_string(),
            },
        ],
        recommended: "defer".to_string(),
    }
}

// 그 확인 지점의 카드 id — 지점 파일과 줄의 글자(글자·숫자·- 만, 80글자). 원본 apHarnessCardId
pub fn harness_card_id(p: &HarnessPoint) -> String {
    let head: String = HARNESS_CARD_PREFIX.to_string() + &p.file + "-" + &p.line.to_string();
    let folded: String = head.chars().map(|c| if c.is_alphabetic() || c.is_numeric() || c == '-' { c } else { '-' }).collect();
    cut_chars(&folded, 80)
}

// 출발 전 자율주행 가지에 판정 시나리오로 커밋해도 되는 자리(7단계 ⑫-7b: 세트 이어 쓰기가 고친 시나리오·가져오기 세션 시나리오·버린 실행에서 되살린 시나리오).
// 저장소 안 상대 경로이고, 하네스(CLAUDE.md·.claude/·.mcp.json 등, 접은 꼴)·.git 자리가 아니어야 한다 — 이어 갈 단위 정의의 시나리오 자리가 하네스를 가리키면
// 시나리오 고침이 하네스를 바꾸는 길이 되므로(사용자 하네스는 출발 준비·실행·끝 어디서도 고치지 않는다, 설계 1장)
pub fn seed_scenario_path_ok(rel: &str) -> bool {
    if !scenario_path_ok(rel) {
        return false;
    }
    let folded = path_key(rel);
    let parts: Vec<&str> = folded.split('/').collect();
    if parts.iter().any(|s| *s == ".git" || *s == ".claude") {
        return false;
    }
    if let Some(last) = parts.last() {
        if ["claude.md", "claude.local.md", ".mcp.json", "agents.md", ".gitmodules", ".gitattributes"].contains(last) {
            return false;
        }
    }
    !harness_file_wanted(rel)
}

// 결과 폴더 안 단위 자리 이름으로 써도 되는 단위 id(⑫-7b 자동 보안 검토 MEDIUM 8: 대기열 단위 id 를 검사 없이 scenarios/<id>/ 로 써서 "../" 로 결과 폴더 밖에 썼다)
pub fn unit_dir_ok(id: &str) -> bool {
    let cs: Vec<char> = id.chars().collect();
    (1..=60).contains(&cs.len())
        && cs.iter().all(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        && id != "."
        && id != ".."
        && !id.starts_with('.')
}

// 지난 실행의 시나리오 고정본 자리를 기록 글자가 아니라 조각으로 다시 만든다(⑫-7b 자동 보안 검토 MEDIUM 7: frozen.json 의 pinned 글자에 ".." 를 넣으면
// 앞자리 검사를 지나 다른 파일을 읽어 커밋에 넣었다). 엔진 start 가 뜨는 자리 그대로: <실행 폴더>/pinned/checks/scenarios/<단위 차례>/<시나리오 차례>/<파일 이름>
pub fn pinned_scenario_path(run_dir: &str, unit: i64, scenario: i64, file: &str) -> Option<String> {
    // NSString lastPathComponent 과 같이 끝의 '/' 는 버리고 마지막 조각을 똔다
    let name = file.trim_end_matches('/').rsplit('/').next().unwrap_or(file);
    if unit < 1
        || scenario < 1
        || name.is_empty()
        || cluster_len(name) > 200
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.chars().any(|c| (c as u32) < 0x20 || (c as u32) == 0x7f)
    {
        return None;
    }
    Some(format!("{}/pinned/checks/scenarios/{}/{}/", run_dir, unit, scenario) + name)
}

// MARK: - 정책 카드 (7단계 ⑫-7b): 확인 지점 가운데 사람 몫 정책이 필요한 것 — 바깥 쓰기, 배포·운영 확인

// 정책 카드는 프로젝트 설정 policy_answers 에 답이 남아 다음 출발에 미리 골라진다(2단계 정책 답 길). 그래서 id 는 줄 번호가 아니라
// 파일·종류·줄 글의 지문으로 둔다(앞에 줄이 늘어도 같은 답을 잇는다). 같은 글이 다른 줄로 옮겨도 같은 카드
pub const HARNESS_POLICY_PREFIX: &str = "harness-policy-";

// 배포·운영 낱말(대소문자 접음)
fn deploy_words() -> &'static Pattern {
    static P: OnceLock<Pattern> = OnceLock::new();
    P.get_or_init(|| Pattern::compile(r"배포|릴리스|릴리즈|운영|프로덕션|deploy|release|production|\bprod\b|마이그레이션|migration", true))
}

// 확인 지점의 정책 종류(바깥 쓰기·배포·운영). 원본 apHarnessPolicyClass
pub fn harness_policy_class(p: &HarnessPoint) -> Option<&'static str> {
    if p.kind == "write" {
        return Some("write");
    }
    if ["confirm", "approve", "block", "wait"].contains(&p.kind.as_str())
        && deploy_words().hits(&p.text.chars().collect::<Vec<char>>())
    {
        return Some("deploy");
    }
    None
}

// FNV-1a 64 지문 — 견주기에만 쓴다(암호 해시가 아니다; 규칙 라이브러리는 암호 해시를 두지 않는다). 원본 apFNV64Data
pub fn fnv64_data(d: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in d {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

// 그 확인 지점의 정책 카드 id. 원본 apHarnessPolicyId
pub fn harness_policy_id(p: &HarnessPoint) -> String {
    let mut seed = String::new();
    seed.push_str(&path_key(&p.file));
    seed.push('\u{1}');
    seed.push_str(&p.kind);
    seed.push('\u{1}');
    seed.push_str(&p.text.nfc().collect::<String>());
    HARNESS_POLICY_PREFIX.to_owned() + &fnv64_data(seed.as_bytes())
}

// 확인 지점의 정책 카드 — 바꾸지 않고 묻고, 답은 프로젝트 설정에 남는다(7단계 ⑫-7b). 원본 apHarnessPolicyCard
pub fn harness_policy_card(p: &HarnessPoint, missing: bool) -> Option<CardText> {
    let class = harness_policy_class(p)?;
    let tail = if missing {
        " 자율주행 지시가 이 자리를 다루지 않았습니다."
    } else {
        " 자율주행 지시가 다루지만, 사람 몫 정책이라 답을 프로젝트 설정에 남겨 다음 출발에도 씁니다."
    };
    if class == "write" {
        return Some(CardText {
            label: "하네스 정책: 바깥 쓰기".to_string(),
            title: format!("하네스가 바깥 쓰기를 시키는 자리를 자율주행에서 어떻게 할까요: {}", p.place()),
            why: format!("{} (자율주행 세션에는 바깥 쓰기 도구가 없고 쓰기는 보드 내장 길로만 합니다.{})", p.text, tail),
            options: vec![
                CardOption { key: "defer".to_string(), label: "보류해 다음 출발로".to_string(), note: "그 부분은 하지 않고 next_start 에 남깁니다".to_string() },
                CardOption { key: "record".to_string(), label: "결과 기록에만 남김".to_string(), note: "올릴 내용을 결과 기록에 쉬운 말로 남기고, 사람이 보고 올립니다".to_string() },
                CardOption { key: "drop".to_string(), label: "범위에서 뺌".to_string(), note: "이 자리가 걸린 일은 이번 실행에서 하지 않습니다".to_string() },
            ],
            recommended: "defer".to_string(),
        });
    }
    Some(CardText {
        label: "하네스 정책: 배포·운영 확인".to_string(),
        title: format!("하네스가 배포·운영 전에 사람 확인을 요구하는 자리를 자율주행에서 어떻게 할까요: {}", p.place()),
        why: format!("{} (대답할 사람이 없어 자율주행은 그 확인을 받을 수 없습니다.{})", p.text, tail),
        options: vec![
            CardOption { key: "defer".to_string(), label: "그 단계 앞에서 보류".to_string(), note: "배포·운영 반영은 하지 않고 next_start 에 남깁니다".to_string() },
            CardOption { key: "drop".to_string(), label: "범위에서 뺌".to_string(), note: "이 자리가 걸린 일은 이번 실행에서 하지 않습니다".to_string() },
        ],
        recommended: "defer".to_string(),
    })
}


// MARK: - 하네스 파일 모으기 (7단계 ⑫-7b)

// 파일마다 한도(256 KB), 개수 한도(300개), 합계 한도(4 MB). 원본 apHarnessFileBytes·apHarnessFileMax·apHarnessTotalBytes
pub const HARNESS_FILE_BYTES: usize = 256 << 10;
pub const HARNESS_FILE_MAX: usize = 300;
pub const HARNESS_TOTAL_BYTES: usize = 4 << 20;

// 어댑터가 읽은 파일 하나 — 보통 파일이면 글, 아니면 종류(link:<대상>·fifo·socket·device·large:<크기>)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessFileRead {
    Data(Vec<u8>),
    Other(String),
}

// 안내를 걸고 다니는 가지 차례(원본 APWalkStep — Ports.swift)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkStep {
    Next,
    Skip,
    Stop,
}

// 하네스 파일을 걷고·읽는 가지(원본 APHarnessSource — 실제 어댑터가 만든다; ports.rs 통합 전까지 이 파일에 둔다)
pub trait HarnessSource {
    // 하네스 파일 후보 상대 경로를 방문 차례로
    fn walk(&self, visit: &mut dyn FnMut(&str) -> WalkStep);
    // max 보다 큰 파일은 Other("large:<크기>") 로 돌려준다
    fn read(&self, rel: &str, max: usize) -> Option<HarnessFileRead>;
}

// 읽은 하네스 파일 하나와 그 지문(FNV-1a 64 — 규칙 라이브러리는 Foundation 만이라 암호 해시 대신, 견주기에만).
// 원본 APHarnessFileMark
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessFileMark {
    pub path: String,
    pub digest: String,
    pub size: i64,
}

// 모은 하네스 글과 지문(원본 APHarnessRead)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessRead {
    // 상대 경로 → 글(보통 파일이고 상한 안)
    pub texts: BTreeMap<String, String>,
    // 지문(읽지 않은 링크·큰 파일도 종류로 든다)
    pub marks: Vec<HarnessFileMark>,
    // "경로: 까닭"
    pub skipped: Vec<String>,
    // 개수·합계 상한에 닿아 멈춤
    pub capped: bool,
}

impl HarnessRead {
    // 글만 — 경로 낱말 차례(원본은 `texts.keys.sorted()`)
    pub fn files(&self) -> Vec<(String, String)> {
        self.texts.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

// 하네스 파일 글과 지문 — harness_file_wanted 가 고른 것만, 링크를 따라가지 않고(어댑터), 상한 안에서.
// 원본 apHarnessCollect
pub fn harness_collect(src: &dyn HarnessSource) -> HarnessRead {
    let mut r = HarnessRead::default();
    let mut total = 0usize;
    src.walk(&mut |rel| {
        let folded = path_key(rel);
        let parts: Vec<&str> = folded.split('/').collect();
        let in_claude = parts[..parts.len().saturating_sub(1)].contains(&".claude");
        if let Some(name) = parts.last() {
            if !in_claude && HARNESS_WALK_SKIP.contains(name) {
                return WalkStep::Skip;
            }
            if in_claude {
                if let Some(ci) = parts.iter().position(|s| *s == ".claude") {
                    if parts.len() == ci + 2 && CLAUDE_SKIP.contains(name) {
                        return WalkStep::Skip;
                    }
                }
            }
            if parts.len() > HARNESS_DEPTH + 1 && !in_claude && *name != ".claude" {
                return WalkStep::Skip;
            }
        }
        if !harness_file_wanted(rel) {
            return WalkStep::Next;
        }
        if r.marks.len() >= HARNESS_FILE_MAX {
            r.capped = true;
            return WalkStep::Stop;
        }
        let this_max = HARNESS_FILE_BYTES.min(HARNESS_TOTAL_BYTES.saturating_sub(total));
        match src.read(rel, this_max) {
            Some(HarnessFileRead::Data(d)) => {
                total += d.len();
                r.marks.push(HarnessFileMark { path: rel.to_string(), digest: fnv64_data(&d), size: d.len() as i64 });
                r.texts.insert(rel.to_string(), String::from_utf8_lossy(&d).into_owned());
            }
            Some(HarnessFileRead::Other(k)) => {
                r.marks.push(HarnessFileMark { path: rel.to_string(), digest: k.clone(), size: 0 });
                let note = if k.starts_with("link") {
                    "바로 가기라 따라가지 않습니다".to_string()
                } else if k.starts_with("large") {
                    "크기 상한(256 KB·합계 4 MB)을 넘습니다".to_string()
                } else {
                    format!("보통 파일이 아닙니다({})", k)
                };
                r.skipped.push(format!("{}: {}", rel, note));
                if k.starts_with("large") && total >= HARNESS_TOTAL_BYTES {
                    r.capped = true;
                }
            }
            None => {}
        }
        WalkStep::Next
    });
    r.marks.sort_by_key(|a| path_key(&a.path));
    r
}

// 옮기기 커밋의 이름 — 글만(엔진 harnessTexts)
pub fn harness_texts(src: &dyn HarnessSource) -> Vec<(String, String)> {
    harness_collect(src).files()
}

// MARK: - 지난 세트 기준과 견주기

// 바뀜·더함·뺌(원본 APHarnessChange)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessChange {
    pub path: String,
    // added·removed·changed
    pub change: String,
}

// 지난 세트 기준과 견주기 — 경로는 접어서(대소문자·정규화), 더함·뺌 모두 낱말 차례로. 원본 apHarnessChanges
pub fn harness_changes(old: &[HarnessFileMark], new: &[HarnessFileMark]) -> Vec<HarnessChange> {
    let mut o: BTreeMap<String, &HarnessFileMark> = BTreeMap::new();
    let mut n: BTreeMap<String, &HarnessFileMark> = BTreeMap::new();
    for m in old {
        o.insert(path_key(&m.path), m);
    }
    for m in new {
        n.insert(path_key(&m.path), m);
    }
    let mut keys: Vec<String> = o.keys().chain(n.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    let mut out: Vec<HarnessChange> = Vec::new();
    for k in keys {
        let (a, b) = (o.get(&k), n.get(&k));
        match (a, b) {
            (None, Some(b)) => out.push(HarnessChange { path: b.path.clone(), change: "added".to_string() }),
            (Some(a), None) => out.push(HarnessChange { path: a.path.clone(), change: "removed".to_string() }),
            (Some(a), Some(b)) if a.digest != b.digest || a.size != b.size => {
                out.push(HarnessChange { path: b.path.clone(), change: "changed".to_string() })
            }
            _ => {}
        }
    }
    out
}

// MARK: - 세트의 하네스 기준

// 세트의 하네스 기준(set/harness.json) — 그 지시가 마지막으로 맞춰진 하네스 지문. 보드가 출발할 때 쓰고 엔진 harness 가 읽는다
pub const HARNESS_BASELINE_FORMAT: &str = "claude-board-harness-baseline";

// 원본 APHarnessBaseline
#[derive(Debug, Clone, PartialEq)]
pub struct HarnessBaseline {
    pub marks: Vec<HarnessFileMark>,
    pub points: i64,
    pub at: f64,
    pub run: String,
}

// 기준 파일의 json 글(원본 apHarnessBaselineJSON — [String: Any] 대신 Value)
pub fn harness_baseline_json(b: &HarnessBaseline) -> Value {
    let mut files: Vec<Value> = Vec::new();
    for m in &b.marks {
        let mut f = Map::new();
        f.insert("path".to_string(), Value::String(m.path.clone()));
        f.insert("digest".to_string(), Value::String(m.digest.clone()));
        f.insert("size".to_string(), Value::Number(m.size.into()));
        files.push(Value::Object(f));
    }
    let mut out = Map::new();
    out.insert("format".to_string(), Value::String(HARNESS_BASELINE_FORMAT.to_string()));
    out.insert("version".to_string(), Value::Number(1.into()));
    out.insert("points".to_string(), Value::Number(b.points.into()));
    out.insert(
        "at".to_string(),
        serde_json::Number::from_f64(b.at).map(Value::Number).unwrap_or(Value::Null),
    );
    out.insert("run".to_string(), Value::String(b.run.clone()));
    out.insert("files".to_string(), Value::Array(files));
    Value::Object(out)
}

// 기준 파일 읽기(원본 apHarnessBaselineParse) — 길이 1 MB 까지, 파일은 최근 600개, 경로와 지문 길이를 검한다
pub fn harness_baseline_parse(d: Option<&[u8]>) -> Option<HarnessBaseline> {
    let d = d?;
    if d.len() > 1 << 20 {
        return None;
    }
    let o: Value = serde_json::from_slice(d).ok()?;
    if o.get("format").and_then(Value::as_str) != Some(HARNESS_BASELINE_FORMAT) {
        return None;
    }
    let fs = o.get("files").and_then(Value::as_array)?;
    let marks: Vec<HarnessFileMark> = fs
        .iter()
        .take(HARNESS_FILE_MAX * 2)
        .filter_map(|f| {
            let path = f.get("path").and_then(Value::as_str)?;
            if !scenario_path_ok(path) {
                return None;
            }
            let digest = f.get("digest").and_then(Value::as_str)?;
            if grapheme_len(digest) > 300 {
                return None;
            }
            let size = f.get("size").and_then(Value::as_i64).unwrap_or(0);
            Some(HarnessFileMark { path: path.to_string(), digest: digest.to_string(), size })
        })
        .collect();
    let points = o.get("points").and_then(Value::as_i64).unwrap_or(0);
    let at = o.get("at").and_then(Value::as_f64).unwrap_or(0.0);
    let run = o.get("run").and_then(Value::as_str).unwrap_or("").to_string();
    Some(HarnessBaseline { marks, points, at, run })
}

// MARK: - 하네스 읽기 상태

// 하네스 읽기 결과(엔진 harness → 준비 폴더 harness.json): 확인 지점·지문·지난 세트 기준과의 차이. 기준이 없으면 그대로로 본다(이번 출발 때 기준이 생김).
// 원본 APHarnessState
#[derive(Debug, Clone, PartialEq)]
pub struct HarnessState {
    pub points: Vec<HarnessPoint>,
    pub marks: Vec<HarnessFileMark>,
    pub changes: Vec<HarnessChange>,
    pub has_baseline: bool,
}

impl HarnessState {
    // 기준이 있고 바뀐 파일이 있는가
    pub fn changed(&self) -> bool {
        self.has_baseline && !self.changes.is_empty()
    }
}

// 하네스 읽기 상태(원본 apHarnessState)
pub fn harness_state(read: &HarnessRead, baseline: Option<&HarnessBaseline>) -> HarnessState {
    HarnessState {
        points: harness_points(&read.files()),
        marks: read.marks.clone(),
        changes: baseline.map(|b| harness_changes(&b.marks, &read.marks)).unwrap_or_default(),
        has_baseline: baseline.is_some(),
    }
}

// 준비 화면 한 줄(원본 apHarnessStateNote)
pub fn harness_state_note(s: &HarnessState, set_has_instruction: bool) -> String {
    let mut kinds: BTreeMap<String, i64> = BTreeMap::new();
    for p in &s.points {
        *kinds.entry(p.kind.clone()).or_insert(0) += 1;
    }
    let mut ks: Vec<String> = Vec::new();
    for k in ["ask", "confirm", "approve", "preview", "wait", "write", "block"] {
        if let Some(n) = kinds.get(k) {
            ks.push(format!("{} {}", harness_kind_label(k), n));
        }
    }
    let mut t = if s.points.is_empty() {
        String::from("확인 지점이 없습니다")
    } else {
        format!("확인 지점 {}곳({})", s.points.len(), ks.join(" · "))
    };
    t.push_str(&format!(" · 파일 {}개", s.marks.len()));
    if !set_has_instruction {
        return t + " · 세트에 지시가 없습니다";
    }
    if !s.has_baseline {
        return t + " · 지난 세트의 하네스 기준이 없어 그대로로 봅니다(이번 출발 때 기준을 남깁니다)";
    }
    if s.changes.is_empty() {
        t.push_str(" · 지난 세트 기준과 같습니다");
    } else {
        t.push_str(&format!(" · 지난 세트 기준과 달라진 파일 {}개", s.changes.len()));
    }
    t
}





