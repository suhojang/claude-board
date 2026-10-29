//! 계정 추가 로그인의 상태 기계 — 입출력 없는 순수 규칙(헥사고날의 도메인 쪽).
//! 로그인 명령의 출력 조각·종료·붙여 넣은 코드·보드가 로그인을 끝냈다는 소식을 받아 "무엇을 할지"(`Effect`)만 돌려준다.
//! ConPTY 에 쓰기·프론트로 알리기·로그인 취소는 바깥(`super`)이 한다.

use super::AccountLoginEvent;
use crate::login_output::LoginOutput;

/// 실패 사유로 보이는 글자 수 상한(mac `prefix(200)`)
const REASON_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// 프론트에 알린다
    Emit(AccountLoginEvent),
    /// 로그인 ConPTY 입력에 쓴다
    Write(Vec<u8>),
    /// 계정 추가를 접고 이 안내를 오류로 띄운다
    Fail(String),
}

#[derive(Debug, Default)]
pub struct Machine {
    output: LoginOutput,
    url_sent: bool,
    exited: bool,
    /// 마지막 소식(`done`)을 이미 알렸다 — 이후 소식은 모두 버린다
    ended: bool,
}

fn emit(event: AccountLoginEvent) -> Effect {
    Effect::Emit(event)
}

impl Machine {
    /// 로그인 명령이 쓴 조각 — 주소가 처음 온전해지는 때 한 번만 알린다.
    pub fn on_output(&mut self, bytes: &[u8]) -> Vec<Effect> {
        if self.ended {
            return Vec::new();
        }
        self.output.push(bytes);
        if self.url_sent {
            return Vec::new();
        }
        match self.output.url() {
            Some(url) => {
                self.url_sent = true;
                vec![emit(AccountLoginEvent { url: Some(url), ..Default::default() })]
            }
            None => Vec::new(),
        }
    }

    /// `BROWSER` 도우미가 주소 파일(`url.txt`)에 적어 둔 글 — 출력에서 주소를 못 찾았을 때의 두 번째 길.
    /// 주소는 처음 온전해지는 때 한 번만 알린다(출력에서 먼저 찾았으면 파일은 보지 않는다).
    pub fn on_url_file(&mut self, text: &str) -> Vec<Effect> {
        if self.ended || self.url_sent {
            return Vec::new();
        }
        // 파일 끝에 줄바꿈이 없어도 주소를 끝까지 쓴 것으로 읽는다(도우미가 한 번에 한 줄을 쓴다)
        match crate::login_output::find_login_url(&format!("{text}\n")) {
            Some(url) => {
                self.url_sent = true;
                vec![emit(AccountLoginEvent { url: Some(url), ..Default::default() })]
            }
            None => Vec::new(),
        }
    }

    /// 브라우저가 보여 준 코드를 명령에 써 넣는다. 제어 문자는 버린다(붙여 넣기에 섞인 Ctrl+C 가 로그인을 끊지 못하게).
    /// 비었거나 명령이 이미 끝났으면 아무것도 하지 않는다.
    pub fn on_submit(&mut self, code: &str) -> Vec<Effect> {
        if self.ended || self.exited {
            return Vec::new();
        }
        let code: String = code.chars().filter(|c| !c.is_control()).collect();
        let code = code.trim();
        if code.is_empty() {
            return Vec::new();
        }
        vec![
            Effect::Write(format!("{code}\r").into_bytes()),
            emit(AccountLoginEvent { note: Some("코드를 보냈습니다. 확인하는 중…".into()), ..Default::default() }),
        ]
    }

    /// 로그인 명령이 끝났다(출력도 끝까지 읽었다). `pending` = 보드가 아직 이 로그인을 기다리는 중이다.
    /// 성공이면 보드의 폴더 지켜보기가 곧 계정 정보를 찾는다. 실패면 마지막 줄을 사유로 접는다.
    pub fn on_exit(&mut self, code: u32, pending: bool) -> Vec<Effect> {
        self.exited = true;
        if self.ended || !pending {
            return Vec::new();
        }
        if code == 0 {
            return vec![emit(AccountLoginEvent { note: Some("로그인을 마쳤습니다. 계정 정보를 확인하는 중…".into()), ..Default::default() })];
        }
        self.ended = true;
        let reason: String = self.output.failure_reason().unwrap_or_default().chars().take(REASON_MAX_CHARS).collect();
        let message = if reason.is_empty() { "계정을 추가하지 못했습니다".to_string() } else { format!("계정을 추가하지 못했습니다: {reason}") };
        vec![Effect::Fail(message.clone()), emit(AccountLoginEvent { done: Some(true), error: Some(message), ..Default::default() })]
    }

    /// 보드가 이 로그인을 끝냈다(계정 저장·취소·시간 초과·로그인 창 닫음) — 프론트에 끝났다고 한 번 알린다.
    pub fn on_ended(&mut self) -> Vec<Effect> {
        if self.ended {
            return Vec::new();
        }
        self.ended = true;
        vec![emit(AccountLoginEvent { done: Some(true), ..Default::default() })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://login.fake.test/oauth/authorize?client_id=fake&state=abc";

    fn url_event(url: &str) -> Effect {
        emit(AccountLoginEvent { url: Some(url.into()), ..Default::default() })
    }

    fn note(text: &str) -> Effect {
        emit(AccountLoginEvent { note: Some(text.into()), ..Default::default() })
    }

    fn banner() -> String {
        format!("Opening browser to sign in...\r\nIf the browser did not open, visit: {URL}\r\n\r\nPaste code here if prompted > ")
    }

    #[test]
    fn url_is_announced_once_when_it_is_whole_even_across_reads() {
        let mut m = Machine::default();
        let text = banner();
        let (head, tail) = text.as_bytes().split_at(text.find("fake.test").unwrap());
        assert_eq!(m.on_output(head), []);
        assert_eq!(m.on_output(tail), [url_event(URL)]);
        // 같은 줄이 다시 나와도 다시 알리지 않는다
        assert_eq!(m.on_output(banner().as_bytes()), []);
    }

    #[test]
    fn the_url_file_is_a_second_source_and_the_url_is_still_announced_once() {
        let mut from_file = Machine::default();
        assert_eq!(from_file.on_url_file(""), []);
        assert_eq!(from_file.on_url_file("not a url\r\n"), []);
        assert_eq!(from_file.on_url_file(&format!("{URL}\r\n")), [url_event(URL)]);
        assert_eq!(from_file.on_url_file(&format!("{URL}\r\n")), [], "두 번째는 알리지 않는다");
        assert_eq!(from_file.on_output(banner().as_bytes()), [], "파일에서 알린 뒤 출력의 주소는 다시 알리지 않는다");
        // 줄바꿈 없이 끝나는 파일도 온전한 주소로 읽는다
        assert_eq!(Machine::default().on_url_file(URL), [url_event(URL)]);
        // 출력에서 먼저 찾았으면 파일은 보지 않는다
        let mut from_output = Machine::default();
        assert_eq!(from_output.on_output(banner().as_bytes()), [url_event(URL)]);
        assert_eq!(from_output.on_url_file(&format!("{URL}\n")), []);
        // 끝난 로그인은 파일도 버린다
        let mut ended = Machine::default();
        ended.on_ended();
        assert_eq!(ended.on_url_file(URL), []);
    }

    #[test]
    fn submitting_a_code_writes_it_with_a_carriage_return_and_notes_it() {
        let mut m = Machine::default();
        m.on_output(banner().as_bytes());
        assert_eq!(m.on_submit("  fake-code-1234 \n"), [Effect::Write(b"fake-code-1234\r".to_vec()), note("코드를 보냈습니다. 확인하는 중…")]);
    }

    #[test]
    fn empty_codes_and_control_characters_are_not_written() {
        let mut m = Machine::default();
        assert_eq!(m.on_submit(""), []);
        assert_eq!(m.on_submit(" \t\r\n"), []);
        assert_eq!(m.on_submit("\u{3}\u{1B}"), []);
        // 붙여 넣기에 섞인 제어 문자는 빼고 보낸다
        assert_eq!(m.on_submit("ab\u{3}cd"), [Effect::Write(b"abcd\r".to_vec()), note("코드를 보냈습니다. 확인하는 중…")]);
    }

    #[test]
    fn a_code_after_the_command_exited_or_the_login_ended_is_dropped() {
        let mut exited = Machine::default();
        exited.on_exit(0, true);
        assert_eq!(exited.on_submit("code"), []);
        let mut ended = Machine::default();
        ended.on_ended();
        assert_eq!(ended.on_submit("code"), []);
    }

    #[test]
    fn exit_zero_waits_for_the_board_to_find_the_account() {
        let mut m = Machine::default();
        m.on_output(banner().as_bytes());
        assert_eq!(m.on_exit(0, true), [note("로그인을 마쳤습니다. 계정 정보를 확인하는 중…")]);
        // 이어서 보드가 끝내면 끝났다고 한 번 알린다
        assert_eq!(m.on_ended(), [emit(AccountLoginEvent { done: Some(true), ..Default::default() })]);
        assert_eq!(m.on_ended(), []);
    }

    #[test]
    fn exit_nonzero_fails_with_the_text_after_the_prompt() {
        let mut m = Machine::default();
        m.on_output(banner().as_bytes());
        m.on_output(b"Login failed: Invalid authorization code\r\n");
        let message = "계정을 추가하지 못했습니다: Login failed: Invalid authorization code".to_string();
        assert_eq!(
            m.on_exit(1, true),
            [Effect::Fail(message.clone()), emit(AccountLoginEvent { done: Some(true), error: Some(message), ..Default::default() })]
        );
        // 이미 알렸다 — 보드가 이어서 접었다고 해도 또 알리지 않는다
        assert_eq!(m.on_ended(), []);
        assert_eq!(m.on_output(b"late"), []);
    }

    #[test]
    fn exit_nonzero_without_any_text_still_fails() {
        let mut m = Machine::default();
        let effects = m.on_exit(2, true);
        assert_eq!(effects[0], Effect::Fail("계정을 추가하지 못했습니다".into()));
    }

    #[test]
    fn a_very_long_reason_is_cut() {
        let mut m = Machine::default();
        m.on_output(format!("prompted > {}\r\n", "가".repeat(500)).as_bytes());
        let Effect::Fail(message) = &m.on_exit(1, true)[0] else { panic!() };
        assert_eq!(message.chars().count(), "계정을 추가하지 못했습니다: ".chars().count() + 200);
    }

    #[test]
    fn exit_after_the_board_gave_up_changes_nothing() {
        // 보드가 이미 접은 로그인(취소·시간 초과) — 프로세스를 죽여 생긴 종료는 실패로 치지 않는다
        let mut m = Machine::default();
        assert_eq!(m.on_exit(1, false), []);
        assert_eq!(m.on_ended().len(), 1);
    }
}
