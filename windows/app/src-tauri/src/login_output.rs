//! `claude auth login --claudeai` 출력 읽기 — 순수 함수만 둔다(mac `ClaudeBoard.swift` 의 `loginOutput`·`loginEnded` 와 같은 규칙).
//!
//! - `strip_ansi` — 색·창 제목 제어열을 걷어 내고, 커서를 칸 사이로 옮기는 제어열(`C`·`G`·`H`·`f`)은 공백 하나로 바꾼다
//!   (화면 글은 커서 이동으로 띄어 쓰이므로 지워 버리면 주소 뒤 글이 주소에 붙는다). 끝이 잘린 제어열은 버린다(다음 읽기에서 온전해지면 그때 걷힌다).
//! - `find_login_url` — `oauth/authorize` 가 든 https 주소를, 뒤에 공백이 올 때만(= 끝까지 읽혔을 때만) 돌려준다.
//! - `failure_reason` — 마지막 비어 있지 않은 줄에서 `prompted >` 뒤의 글.
//! - `LoginOutput` — 읽기 조각(바이트)을 모아 위 둘을 묻는 자리. 글자·제어열·주소가 읽기 경계에서 잘려도 모았다가 읽는다.
//!
//! 한계(unchecked): 진짜 Windows `claude auth login` 의 ConPTY 출력 모양으로 재 보지 못했다. 시험 입력은 mac 파서와 가짜 로그인(`windows/ci/fake-claude-login.cjs`)에서 따왔다.

/// 모아 두는 글자 상한 — 넘으면 뒤쪽 절반만 남긴다(mac 은 글자 수 20,000 → 10,000)
const KEEP_MAX_BYTES: usize = 20_000;
const KEEP_AFTER_TRIM_BYTES: usize = 10_000;

/// 제어열을 걷어 낸 글. CSI(`ESC [ … 끝글자`)·OSC(`ESC ] … BEL` 또는 `… ESC \`)·`ESC (` 같은 짧은 열을 지운다.
/// 커서 이동 CSI(끝글자 `C`·`G`·`H`·`f`)는 지우지 않고 공백 하나로 바꾼다.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1B}' => {
                if skip_escape(&mut chars) {
                    out.push(' ');
                }
            }
            '\u{07}' => {}
            other => out.push(other),
        }
    }
    out
}

/// `ESC` 바로 뒤에서 호출 — 제어열 한 개를 끝까지 먹는다. 끝이 없으면 남은 글을 모두 먹는다.
/// 커서를 칸 사이로 옮기는 CSI(`C`·`G`·`H`·`f`)를 먹었으면 true — 호출한 쪽이 그 자리에 공백을 둔다.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    match chars.peek().copied() {
        Some('[') => {
            chars.next();
            // 매개변수·중간 글자 뒤 끝글자(0x40-0x7E) 하나
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7E}').contains(&c) {
                    return matches!(c, 'C' | 'G' | 'H' | 'f');
                }
            }
            false
        }
        Some(']') => {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '\u{07}' {
                    break;
                }
                if c == '\u{1B}' {
                    // ST(`ESC \`)
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
            false
        }
        Some(_) => {
            // `ESC ( B`·`ESC =`·`ESC 7` — 중간 글자(0x20-0x2F)들 뒤 끝글자 하나
            while let Some(&c) = chars.peek() {
                chars.next();
                if !('\u{20}'..='\u{2F}').contains(&c) {
                    break;
                }
            }
            false
        }
        None => false,
    }
}

/// 로그인 주소 — 출력에서 처음 찾은 것. https 로 시작해 `oauth/authorize` 를 품은 공백 없는 덩어리이고 뒤에 공백이 있어야 한다.
pub fn find_login_url(text: &str) -> Option<String> {
    let clean = strip_ansi(text);
    const SCHEME: &str = "https://";
    const MARK: &str = "oauth/authorize";
    let mut rest = clean.as_str();
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        let skipped = rest.len() - trimmed.len();
        let Some(end) = trimmed.find(char::is_whitespace) else {
            // 덩어리 뒤에 공백이 없다 — 아직 읽는 중일 수 있으니 주소로 치지 않는다
            return None;
        };
        let token = &trimmed[..end];
        if let Some(url) = url_in_token(token, SCHEME, MARK) {
            return Some(url.to_string());
        }
        rest = &rest[skipped + end..];
    }
    None
}

/// 덩어리 안에서 가장 앞선 `https://` 부터, 그 뒤에 한 글자 이상 건너 `oauth/authorize` 가 나오는 곳.
fn url_in_token<'a>(token: &'a str, scheme: &str, mark: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(found) = token[from..].find(scheme) {
        let start = from + found;
        let after = &token[start + scheme.len()..];
        if after.find(mark).is_some_and(|at| at >= 1) {
            return Some(&token[start..]);
        }
        from = start + scheme.len();
    }
    None
}

/// 로그인이 막혔을 때의 사유 — 마지막 비어 있지 않은 줄에서 코드 입력 안내(`prompted >`)를 뗀 글.
/// 실패 사유는 안내와 같은 줄에 이어 붙어 나온다(`Paste code here if prompted > Login failed: …`).
pub fn failure_reason(text: &str) -> Option<String> {
    let clean = strip_ansi(text);
    clean
        .split(is_line_break)
        .map(|line| line.rsplit("prompted >").next().unwrap_or(line).trim().to_string())
        .rfind(|line| !line.is_empty())
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// 읽기 조각(바이트)을 모으는 자리.
#[derive(Debug, Default)]
pub struct LoginOutput {
    raw: String,
    /// 글자 중간에서 끊겨 다음 조각을 기다리는 바이트
    pending: Vec<u8>,
}

impl LoginOutput {
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let mut bytes = std::mem::take(&mut self.pending);
        loop {
            match std::str::from_utf8(&bytes) {
                Ok(valid) => {
                    self.raw.push_str(valid);
                    break;
                }
                Err(e) => {
                    let (valid, tail) = bytes.split_at(e.valid_up_to());
                    // valid_up_to 앞은 올바른 UTF-8 이라 손실 없이 옮겨진다
                    self.raw.push_str(&String::from_utf8_lossy(valid));
                    match e.error_len() {
                        // 끝이 잘렸다 — 다음 조각과 이어 읽는다
                        None => {
                            self.pending = tail.to_vec();
                            break;
                        }
                        Some(bad) => {
                            self.raw.push('\u{FFFD}');
                            bytes = tail[bad..].to_vec();
                        }
                    }
                }
            }
        }
        self.trim();
    }

    fn trim(&mut self) {
        if self.raw.len() <= KEEP_MAX_BYTES {
            return;
        }
        let mut cut = self.raw.len() - KEEP_AFTER_TRIM_BYTES;
        while !self.raw.is_char_boundary(cut) {
            cut += 1;
        }
        self.raw.drain(..cut);
    }

    pub fn url(&self) -> Option<String> {
        find_login_url(&self.raw)
    }

    pub fn failure_reason(&self) -> Option<String> {
        failure_reason(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://claude.ai/oauth/authorize?code=true&client_id=abc&response_type=code&scope=user%3Aprofile&state=xyz";

    #[test]
    fn strip_ansi_removes_colors_cursor_moves_and_titles() {
        assert_eq!(strip_ansi("\u{1B}[1;32mOK\u{1B}[0m done"), "OK done");
        assert_eq!(strip_ansi("a\u{1B}[2K\u{1B}[1;1Hb\u{1B}[?25lc\u{1B}[?25h"), "a bc");
        assert_eq!(strip_ansi("\u{1B}]0;claude\u{07}text"), "text");
        assert_eq!(strip_ansi("\u{1B}]0;claude\u{1B}\\text"), "text");
        assert_eq!(strip_ansi("\u{1B}(Bplain\u{1B}=\u{1B}7"), "plain");
        assert_eq!(strip_ansi("bell\u{07}"), "bell");
        assert_eq!(strip_ansi("줄\r\n바꿈"), "줄\r\n바꿈");
    }

    #[test]
    fn strip_ansi_turns_cursor_moves_into_one_space() {
        assert_eq!(strip_ansi("a\u{1B}[3Cb"), "a b", "커서 앞으로");
        assert_eq!(strip_ansi("a\u{1B}[Cb"), "a b", "매개변수 없는 앞으로");
        assert_eq!(strip_ansi("a\u{1B}[40Gb"), "a b", "열 지정");
        assert_eq!(strip_ansi("a\u{1B}[2;5Hb"), "a b", "위치 지정");
        assert_eq!(strip_ansi("a\u{1B}[Hb"), "a b");
        assert_eq!(strip_ansi("a\u{1B}[2;5fb"), "a b");
        // 색·지우기·커서 숨김·위로 이동은 공백이 되지 않는다
        assert_eq!(strip_ansi("a\u{1B}[0mb\u{1B}[2Kc\u{1B}[?25ld\u{1B}[2Ae"), "abcde");
        // 이동은 하나마다 공백 하나
        assert_eq!(strip_ansi("a\u{1B}[2C\u{1B}[3Cb"), "a  b");
    }

    #[test]
    fn a_url_followed_by_a_cursor_move_and_text_ends_at_the_move() {
        let text = format!("visit: {URL}\u{1B}[3CPaste code here");
        assert_eq!(find_login_url(&text).as_deref(), Some(URL));
        let text = format!("visit: \u{1B}[4m{URL}\u{1B}[0m\u{1B}[1;60Hnext");
        assert_eq!(find_login_url(&text).as_deref(), Some(URL));
        // 이동 뒤에 글이 아직 없어도 이동이 주소를 끝낸다
        assert_eq!(find_login_url(&format!("visit: {URL}\u{1B}[3C")).as_deref(), Some(URL));
        // 어디서 잘려 읽혀도 같은 주소
        let bytes = format!("visit: {URL}\u{1B}[3Ctext").into_bytes();
        for cut in 1..bytes.len() {
            let mut out = LoginOutput::default();
            out.push(&bytes[..cut]);
            out.push(&bytes[cut..]);
            assert_eq!(out.url().as_deref(), Some(URL), "cut {cut}");
        }
    }

    #[test]
    fn strip_ansi_drops_a_sequence_cut_at_the_end() {
        assert_eq!(strip_ansi("abc\u{1B}[1;3"), "abc");
        assert_eq!(strip_ansi("abc\u{1B}]0;ti"), "abc");
        assert_eq!(strip_ansi("abc\u{1B}"), "abc");
    }

    #[test]
    fn finds_the_url_after_the_hint_text_with_ansi_around_it() {
        let text = format!("Opening browser to sign in...\r\n\u{1B}[2mIf the browser did not open, visit: \u{1B}[0m\u{1B}[4m{URL}\u{1B}[0m\r\n\r\nPaste code here if prompted > ");
        assert_eq!(find_login_url(&text).as_deref(), Some(URL));
    }

    #[test]
    fn url_needs_a_whitespace_after_it() {
        // 읽는 중 — 끝에 공백이 없으면 아직 주소가 아니다
        assert_eq!(find_login_url(&format!("visit: {URL}")), None);
        assert_eq!(find_login_url(&format!("visit: {URL}\r")), Some(URL.to_string()));
        assert_eq!(find_login_url(&format!("visit: {URL} ")), Some(URL.to_string()));
    }

    #[test]
    fn url_must_be_https_and_carry_oauth_authorize() {
        assert_eq!(find_login_url("http://claude.ai/oauth/authorize?x=1 "), None);
        assert_eq!(find_login_url("https://claude.ai/login?x=1 "), None);
        // `https://` 바로 뒤가 oauth/authorize 면 한 글자도 건너지 못해 아니다(mac 정규식의 `[^\s]+`)
        assert_eq!(find_login_url("https://oauth/authorize "), None);
        // 앞선 덩어리가 아니어도 뒤에서 찾는다
        assert_eq!(
            find_login_url("see https://claude.ai/docs then https://login.fake.test/oauth/authorize?s=1 \r\n").as_deref(),
            Some("https://login.fake.test/oauth/authorize?s=1")
        );
    }

    #[test]
    fn url_starts_at_the_first_https_inside_a_token() {
        assert_eq!(find_login_url("(https://a.test/oauth/authorize?r=https://b.test/cb) ").as_deref(), Some("https://a.test/oauth/authorize?r=https://b.test/cb)"));
        assert_eq!(find_login_url("visit:https://a.test/oauth/authorize?z=1 ").as_deref(), Some("https://a.test/oauth/authorize?z=1"));
    }

    #[test]
    fn fake_login_script_output_is_read_like_the_mac_parser_reads_it() {
        // windows/ci/fake-claude-login.cjs 가 쓰는 줄 그대로
        let fake = "Opening browser to sign in...\r\nIf the browser did not open, visit: https://login.fake.test/oauth/authorize?client_id=fake&response_type=code&scope=user%3Aprofile&state=0123456789abcdef\r\n\r\nPaste code here if prompted > ";
        assert_eq!(
            find_login_url(fake).as_deref(),
            Some("https://login.fake.test/oauth/authorize?client_id=fake&response_type=code&scope=user%3Aprofile&state=0123456789abcdef")
        );
        // 안내만 남은 끝줄은 건너뛰어 앞 줄이 사유가 된다(mac 과 같다)
        assert!(failure_reason(fake).unwrap().starts_with("If the browser did not open"));
        let failed = format!("{fake}Login failed: Invalid authorization code\r\n");
        assert_eq!(failure_reason(&failed).as_deref(), Some("Login failed: Invalid authorization code"));
    }

    #[test]
    fn failure_reason_is_the_text_after_the_prompt_on_the_last_non_empty_line() {
        assert_eq!(failure_reason("Paste code here if prompted > Login failed: bad\r\n\r\n").as_deref(), Some("Login failed: bad"));
        assert_eq!(failure_reason("line one\nline two  \n\n").as_deref(), Some("line two"));
        // 안내가 두 번 나와도 마지막 안내 뒤
        assert_eq!(failure_reason("prompted > a prompted > b").as_deref(), Some("b"));
        // 안내 뒤가 비면 그 줄은 건너뛰고 앞 줄
        assert_eq!(failure_reason("Error: x\nPaste code here if prompted > \n").as_deref(), Some("Error: x"));
        assert_eq!(failure_reason("\u{1B}[31mLogin failed: red\u{1B}[0m").as_deref(), Some("Login failed: red"));
        assert_eq!(failure_reason(""), None);
        assert_eq!(failure_reason("\r\n  \r\n"), None);
    }

    #[test]
    fn output_finds_a_url_split_across_reads_and_inside_an_escape() {
        let text = format!("visit: \u{1B}[4m{URL}\u{1B}[0m\r\nnext");
        let bytes = text.as_bytes();
        // 어디서 잘려도(제어열·주소·줄바꿈 한가운데) 다 읽으면 같은 주소가 나온다
        for cut in 1..bytes.len() {
            let mut out = LoginOutput::default();
            out.push(&bytes[..cut]);
            let early = out.url();
            out.push(&bytes[cut..]);
            assert_eq!(out.url().as_deref(), Some(URL), "cut {cut}");
            // 주소 끝에 공백이 닿기 전에는 잘린 주소를 내지 않는다
            if let Some(early) = early {
                assert_eq!(early, URL, "cut {cut}");
            }
        }
    }

    #[test]
    fn output_keeps_a_korean_letter_cut_between_reads() {
        let bytes = "Login failed: 코드가 틀렸다\r\n".as_bytes();
        for cut in 1..bytes.len() {
            let mut out = LoginOutput::default();
            out.push(&bytes[..cut]);
            out.push(&bytes[cut..]);
            assert_eq!(out.failure_reason().as_deref(), Some("Login failed: 코드가 틀렸다"), "cut {cut}");
        }
        // 깨진 바이트는 대체 글자로 두고 멈추지 않는다
        let mut out = LoginOutput::default();
        out.push(b"a\xFFb\r\n");
        assert_eq!(out.failure_reason().as_deref(), Some("a\u{FFFD}b"));
    }

    #[test]
    fn output_trims_old_text_but_keeps_the_end() {
        let mut out = LoginOutput::default();
        for _ in 0..30 {
            out.push("x".repeat(1_000).as_bytes());
        }
        out.push(b"\r\nLogin failed: late\r\n");
        assert!(out.raw.len() <= KEEP_MAX_BYTES);
        assert_eq!(out.failure_reason().as_deref(), Some("Login failed: late"));
    }
}
