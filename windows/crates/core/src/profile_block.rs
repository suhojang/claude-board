//! PowerShell 프로필(`$PROFILE.CurrentUserAllHosts`)에 표지 블록을 넣고 빼는 순수 함수 (macOS `user-setup.sh` 의 zshrc 블록 이식).
//!
//! 파일을 읽고 쓰지 않는다. 문자열을 받아 문자열을 돌려준다.
//!
//! 규칙
//! - 표지 줄은 줄 전체(앞뒤 공백 제외)가 `BEGIN`·`END` 와 같은 줄이다. 따옴표 안이나 다른 글 뒤에 붙은 표지 글자는 표지가 아니다.
//! - 짝은 `END` 줄마다 그보다 앞의 가장 가까운 `BEGIN` 줄과 짓는다. 짝 없는 `BEGIN`·`END` 는 사용자 글로 두고 건드리지 않는다.
//! - `insert_block` 은 멱등이다. 블록이 있으면 그 자리에서 본문만 바꾸고(같으면 글자 하나 안 바뀐다), 둘 이상이면 첫 블록만 남긴다. 없으면 맨 끝에 붙인다.
//! - 붙일 때는 빈 줄 하나를 사이에 둔다. `remove_block` 은 그 빈 줄(블록 바로 앞의 빈 줄, 단 파일 첫 줄이 아닐 때)도 함께 지워
//!   끝이 줄바꿈으로 끝나는 파일은 넣었다 빼면 원래대로 돌아온다.
//! - 사용자 줄은 한 글자도 바꾸지 않는다: 줄바꿈(LF·CRLF 섞임), UTF-8 BOM, 끝 줄바꿈 없음을 그대로 둔다.
//!   예외는 끝 줄바꿈이 없는 파일에 붙일 때 마지막 줄에 줄바꿈이 생기는 것 하나뿐이다.
//! - 새로 붙이는 블록의 줄바꿈은 파일의 마지막 줄바꿈을 따르고, 줄바꿈이 하나도 없으면 CRLF 다.
//!
//! 한계: PowerShell 의 여러 줄 here-string(`@' … '@`)이나 `<# … #>` 주석 안에 표지가 한 줄로 따로 있으면 구별하지 못하고 표지로 본다.

/// 블록 시작 줄(macOS 와 같은 표지).
pub const BEGIN: &str = "# >>> claude-board >>>";
/// 블록 끝 줄.
pub const END: &str = "# <<< claude-board <<<";

const BOM: char = '\u{FEFF}';
const DEFAULT_EOL: &str = "\r\n";

/// 줄 하나: 내용과 줄바꿈(`"\n"`·`"\r\n"`·마지막 줄이면 빈 문자열).
#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    eol: &'a str,
}

fn split_lines(s: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                let (head, tail) = rest.split_at(i + 1);
                let (text, eol) = match head.strip_suffix("\r\n") {
                    Some(t) => (t, "\r\n"),
                    None => (&head[..i], "\n"),
                };
                out.push(Line { text, eol });
                rest = tail;
            }
            None => {
                out.push(Line { text: rest, eol: "" });
                break;
            }
        }
    }
    out
}

fn is_begin(l: &Line) -> bool {
    l.text.trim() == BEGIN
}

fn is_end(l: &Line) -> bool {
    l.text.trim() == END
}

/// 짝이 맞는 블록의 (시작 줄 번호, 끝 줄 번호) 목록.
fn find_blocks(lines: &[Line]) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    let mut open: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        if is_begin(l) {
            open = Some(i);
        } else if is_end(l) {
            if let Some(b) = open.take() {
                blocks.push((b, i));
            }
        }
    }
    blocks
}

fn split_bom(s: &str) -> (&str, &str) {
    match s.strip_prefix(BOM) {
        Some(rest) => (&s[..BOM.len_utf8()], rest),
        None => ("", s),
    }
}

/// 본문 줄 목록. 표지와 같은 줄은 블록을 깨므로 뺀다.
fn body_lines(body: &str) -> Vec<&str> {
    split_lines(body)
        .into_iter()
        .map(|l| l.text)
        .filter(|t| !(t.trim() == BEGIN || t.trim() == END))
        .collect()
}

fn push_all(out: &mut String, lines: &[Line]) {
    for l in lines {
        out.push_str(l.text);
        out.push_str(l.eol);
    }
}

/// 프로필 글에 블록을 넣는다(있으면 본문을 바꾼다). `block_body` 는 표지 줄 사이에 들어갈 줄들이다.
pub fn insert_block(profile_text: &str, block_body: &str) -> String {
    let (bom, text) = split_bom(profile_text);
    let lines = split_lines(text);
    let blocks = find_blocks(&lines);
    let body = body_lines(block_body);
    let mut out = String::with_capacity(profile_text.len() + block_body.len() + 64);
    out.push_str(bom);

    if let Some(&(first_b, first_e)) = blocks.first() {
        let drop = drop_flags(&lines, &blocks[1..]);
        let eol = lines[first_b].eol;
        for (i, l) in lines.iter().enumerate() {
            if drop[i] {
                continue;
            }
            if i == first_b {
                out.push_str(l.text);
                out.push_str(l.eol);
                for b in &body {
                    out.push_str(b);
                    out.push_str(eol);
                }
            } else if i > first_b && i < first_e {
                // 옛 본문 줄은 버린다
            } else {
                out.push_str(l.text);
                out.push_str(l.eol);
            }
        }
        return out;
    }

    let eol = lines.iter().rev().map(|l| l.eol).find(|e| !e.is_empty()).unwrap_or(DEFAULT_EOL);
    push_all(&mut out, &lines);
    if let Some(last) = lines.last() {
        if last.eol.is_empty() {
            out.push_str(eol);
        }
        out.push_str(eol); // 사이의 빈 줄
    }
    out.push_str(BEGIN);
    out.push_str(eol);
    for b in &body {
        out.push_str(b);
        out.push_str(eol);
    }
    out.push_str(END);
    out.push_str(eol);
    out
}

/// 프로필 글에서 블록을 모두 뺀다. 블록이 없으면 입력 그대로다.
pub fn remove_block(profile_text: &str) -> String {
    let (bom, text) = split_bom(profile_text);
    let lines = split_lines(text);
    let blocks = find_blocks(&lines);
    if blocks.is_empty() {
        return profile_text.to_string();
    }
    let drop = drop_flags(&lines, &blocks);
    let mut out = String::with_capacity(profile_text.len());
    out.push_str(bom);
    for (i, l) in lines.iter().enumerate() {
        if !drop[i] {
            out.push_str(l.text);
            out.push_str(l.eol);
        }
    }
    out
}

/// 주어진 블록들의 줄과, 각 블록 바로 앞의 빈 줄(파일 첫 줄이 아닐 때)을 지울 표시.
fn drop_flags(lines: &[Line], blocks: &[(usize, usize)]) -> Vec<bool> {
    let mut drop = vec![false; lines.len()];
    for &(b, e) in blocks {
        for d in &mut drop[b..=e] {
            *d = true;
        }
        if b >= 2 && lines[b - 1].text.is_empty() && !lines[b - 1].eol.is_empty() {
            drop[b - 1] = true;
        }
    }
    drop
}
