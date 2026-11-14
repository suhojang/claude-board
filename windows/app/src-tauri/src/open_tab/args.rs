//! `ClaudeBoard --open-tab` 규칙 — 명령줄 읽기, 요청·답 꼴, 요청 폴더·오래된 요청.
//! 원본(macOS): `Sources/BoardRules/OpenTab.swift` 1-139 줄. 시험은 `Tests/BoardRulesTests/OpenTabTests.swift` 의
//! 명령줄 읽기·답 JSON·요청 폴더/오래된 요청 표를 옮긴 것(경로는 Windows 꼴로 바꿨다).
//! 요청 폴더 약속(CLAUDE.md 데이터 흐름 표): `<요청 id>.req.json`(`id`·`cwd`·`title`·`argv`·`at`,
//! `--no-focus` 일 때만 `noFocus: true`), 답 `<요청 id>.reply.json`(`ok`·`status`·`tab`·`session`·`pid`·`why`).
//! 답 프로세스의 끝 코드: ok 면 0, 아니면 1(원본 `Sources/ClaudeBoard/OpenTab.swift:19`) — 부른 쪽이 본다.
//!
//! C2.2b(`plan.rs` — 요청 검사·탭 가리기)와 C2.3(`close.rs`)이 아직 이 모듈의 값을 안 쓴다 — 연결하면 아래 `allow(dead_code)` 를 지운다.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 쓰는 법 안내 — 실패 답의 `why` 끝에 붙는다
pub const OPEN_TAB_USAGE: &str =
    "쓰는 법: ClaudeBoard --open-tab --cwd <폴더> [--title <탭 이름>] [--launch] [--no-focus] [--timeout <초>] -- claude <인자…>";

/// 답을 기다리는 기본 초 — 보드가 꺼져 있어 켜고 열 때(--launch)는 되살리기(최대 5초 기다림)까지 더 기다린다
pub fn open_tab_default_timeout(launch: bool) -> f64 {
    if launch { 40.0 } else { 20.0 }
}

/// `ClaudeBoard --open-tab` 의 옵션(-- 앞)과 띄울 claude 인자(-- 뒤)
#[derive(Debug, Clone, PartialEq)]
pub struct OpenTabOptions {
    pub cwd: String,
    pub title: Option<String>,
    /// 보드가 꺼져 있으면 켜고 연다(없으면 "보드 꺼짐" 으로 실패)
    pub launch: bool,
    /// 보드의 답을 기다리는 초
    pub timeout: f64,
    pub argv: Vec<String>,
    /// --no-focus: 보드를 앞으로 가져오지 않고 탭 선택·키보드 포커스도 그대로 둔 채 뒤에서 연다
    pub no_focus: bool,
}

/// `--open-tab` 뒤의 인자를 읽는다. 상대 경로 --cwd 는 here(명령을 부른 폴더) 기준으로 붙인다(.·.. 는 앱이 풀어 견준다).
/// 실패는 모두 `OpenTabFailure::BadArgs` 다 — 이 단계에서는 요청을 남기지 않고 끝난다.
pub fn parse_open_tab_args(args: &[String], here: &Path) -> Result<OpenTabOptions, OpenTabFailure> {
    let mut cwd: Option<String> = None;
    let mut title: Option<String> = None;
    let mut launch = false;
    let mut no_focus = false;
    let mut timeout: Option<f64> = None;
    let mut index = 0usize;

    while index < args.len() && args[index] != "--" {
        let (flag, inline) = split_inline_flag(&args[index]);
        match flag {
            "--cwd" => cwd = Some(flag_value(flag, inline, args, &mut index)?),
            "--title" => title = Some(flag_value(flag, inline, args, &mut index)?),
            "--timeout" => {
                let value = flag_value(flag, inline, args, &mut index)?;
                timeout = Some(parse_timeout_seconds(&value)?);
            }
            "--launch" => {
                if inline.is_some() {
                    return Err(flag_without_value(flag));
                }
                launch = true;
            }
            "--no-focus" => {
                if inline.is_some() {
                    return Err(flag_without_value(flag));
                }
                no_focus = true;
            }
            _ => return Err(unknown_option(&args[index])),
        }
        index += 1;
    }

    if index >= args.len() {
        return Err(missing_separator());
    }
    let cwd_value = cwd.filter(|value| !value.is_empty()).ok_or_else(missing_cwd)?;
    // 절대 경로는 그대로, 상대 경로는 부른 폴더 기준으로 붙인다
    let cwd_full = if Path::new(&cwd_value).is_absolute() {
        cwd_value
    } else {
        here.join(cwd_value).to_string_lossy().into_owned()
    };
    Ok(OpenTabOptions {
        cwd: cwd_full,
        title,
        launch,
        timeout: timeout.unwrap_or_else(|| open_tab_default_timeout(launch)),
        argv: args[index + 1..].to_vec(),
        no_focus,
    })
}

/// `--flag=값` 꼴을 깃발과 그 자리의 값으로 나눈다. `--` 로 시작하지 않은 인자의 `=` 는 나누지 않는다
/// (그런 인자는 아래에서 모르는 옵션으로 거절된다). 값이 빈 `--flag=` 도 값이 있는 것으로 본다(빈 값) — Swift 와 같다
fn split_inline_flag(arg: &str) -> (&str, Option<&str>) {
    match arg.find('=') {
        Some(eq) if arg.starts_with("--") => (&arg[..eq], Some(&arg[eq + 1..])),
        _ => (arg, None),
    }
}

/// 깃발의 값을 읽는다 — `--flag=값` 꼴이면 그 값, 아니면 다음 인자(-- 가 아니어야). 값을 읽었으면 그 인자까지 먹고 간다
fn flag_value(flag: &str, inline: Option<&str>, args: &[String], index: &mut usize) -> Result<String, OpenTabFailure> {
    if let Some(value) = inline {
        return Ok(value.to_string());
    }
    let next = *index + 1;
    if next >= args.len() || args[next] == "--" {
        return Err(OpenTabFailure::BadArgs(format!("{flag} 뒤에 값이 없습니다. {OPEN_TAB_USAGE}")));
    }
    *index = next;
    Ok(args[next].clone())
}

/// --timeout 값 — 유한한 1~600 초만 받는다(Swift `Double(v).isFinite, 1...600`)
fn parse_timeout_seconds(value: &str) -> Result<f64, OpenTabFailure> {
    match value.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && (1.0..=600.0).contains(&seconds) => Ok(seconds),
        _ => Err(OpenTabFailure::BadArgs(format!("--timeout 은 1~600 초입니다: {value}"))),
    }
}

fn flag_without_value(flag: &str) -> OpenTabFailure {
    OpenTabFailure::BadArgs(format!("{flag} 는 값을 받지 않습니다"))
}

fn unknown_option(arg: &str) -> OpenTabFailure {
    OpenTabFailure::BadArgs(format!("모르는 옵션입니다: {arg}. {OPEN_TAB_USAGE}"))
}

fn missing_separator() -> OpenTabFailure {
    OpenTabFailure::BadArgs(format!("띄울 명령 앞에 -- 가 없습니다. {OPEN_TAB_USAGE}"))
}

fn missing_cwd() -> OpenTabFailure {
    OpenTabFailure::BadArgs(format!("--cwd 가 없습니다. {OPEN_TAB_USAGE}"))
}

/// 요청 파일(<요청 id>.req.json) — 명령줄 도구가 쓰고 보드가 읽은 뒤 지운다.
/// 모르는 키는 무시된다(옛 보드가 새 칸을 무시하는 것과 같다) — serde 기본값
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTabRequest {
    pub id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub argv: Vec<String>,
    /// 만든 시각(1970 기준 초)
    pub at: f64,
    /// --no-focus 로 뒤에서 열기 — 켤 때만 true, 끄면 None 이라 인코딩한 JSON 에 키가 없다(3.3.3 까지의 요청과 같은 JSON).
    /// 이 칸이 없는 옛 명령줄 도구의 요청은 None(지금 동작), 옛 보드(3.3.2·3.3.3)는 모르는 키를 무시해 요청은 받고 선택만 듣지 않는다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_focus: Option<bool>,
}

impl OpenTabRequest {
    /// 보드를 앞으로 가져오고 그 탭을 골라 키보드 포커스를 옮기는가(opened·exists·reopened 모두) — --no-focus 가 아니면
    pub fn reveals(&self) -> bool {
        self.no_focus != Some(true)
    }
}

/// 답(<요청 id>.reply.json, 명령줄 도구가 한 줄 JSON 으로 그대로 찍는다). status 는 opened(새 탭)·exists(이미 도는 탭을 앞으로)·
/// reopened(claude 가 끝나 셸만 남았거나 되살린 뒤 아직 들어가지 않은 같은 세션·이름의 탭에 다시 띄움), 실패는 bad-args·board-off·refused·timeout.
/// 칸 declaration 차례가 곧 JSON 키 차례다 — 답 키는 알파벳 차례로 고정(ok·pid·session·status·tab·why)이어야 한다
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenTabReply {
    pub ok: bool,
    /// 이미 도는 claude 의 pid(exists 일 때만 — 새로 띄운 claude 는 셸이 뜬 뒤에 생긴다)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    /// 세션 id(아는 때만 — --continue·고르기 --resume 은 띄운 뒤에야 안다)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub status: String,
    /// 탭 이름
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl OpenTabReply {
    /// 한 줄 JSON — 키 차례 고정(알파벳), 빗금·한글 그대로
    pub fn line(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"ok":false,"status":"error"}"#.to_string())
    }
}

/// 명령줄 도구의 실패 — 답 꼴(`OpenTabReply`)로 다시 쓴다. 프로세스는 답이 ok 가 아니면 1 로 끝난다
#[derive(Debug, Clone, PartialEq)]
pub enum OpenTabFailure {
    BadArgs(String),
    BoardOff(String),
    Refused(String),
    Timeout(String),
}

impl OpenTabFailure {
    pub fn reply(&self) -> OpenTabReply {
        let (status, why) = match self {
            OpenTabFailure::BadArgs(why) => ("bad-args", why),
            OpenTabFailure::BoardOff(why) => ("board-off", why),
            OpenTabFailure::Refused(why) => ("refused", why),
            OpenTabFailure::Timeout(why) => ("timeout", why),
        };
        OpenTabReply {
            ok: false,
            pid: None,
            session: None,
            status: status.to_string(),
            tab: None,
            why: Some(why.clone()),
        }
    }
}

/// 요청 폴더 — 사용자만 쓰는 <local app data>\claude-board\open-tab(시험 빌드는 claude-board-dev).
/// 설치판·시험 빌드가 따로다(잠금 파일·분산 알림 이름처럼).
/// Claude Code 샌드박스·자율주행 모델 세션의 쓰기 범위 밖이라 그 안에서 돌린 명령은 요청을 남기지 못한다
pub fn open_tab_dir(local_app_data: &Path, dev: bool) -> PathBuf {
    let board_dir = if dev { "claude-board-dev" } else { "claude-board" };
    local_app_data.join(board_dir).join("open-tab")
}

/// 보드가 받지 않을 만큼 오래된 요청인가 — 명령줄 도구가 기다림을 그만두고 떠난 요청을 보드가 나중에 켜져 띄우지
/// 않게(기다림 상한 600초, 뒤로 넘어간 시계 60초)
pub fn open_tab_stale(at: f64, now: f64) -> bool {
    now - at > 600.0 || at - now > 60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 명령줄 읽기 표(원본 OpenTabTests.swift `args(_:)` — 경로를 Windows 꼴로 바꿨다). want 가 None 이면 거절이 기대다
    struct ArgsRow {
        note: &'static str,
        args: Vec<&'static str>,
        want: Option<OpenTabOptions>,
        why: Option<&'static str>,
    }

    fn expected(cwd: &str, title: Option<&str>, launch: bool, timeout: f64, argv: &[&str], no_focus: bool) -> Option<OpenTabOptions> {
        Some(OpenTabOptions {
            cwd: cwd.to_string(),
            title: title.map(Into::into),
            launch,
            timeout,
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            no_focus,
        })
    }

    fn args_rows() -> Vec<ArgsRow> {
        vec![
            ArgsRow { note: "보통", args: vec!["--cwd", "C:\\w", "--title", "역할", "--", "claude", "--resume", "x"],
                      want: expected("C:\\w", Some("역할"), false, 20.0, &["claude", "--resume", "x"], false), why: None },
            ArgsRow { note: "= 꼴·--launch 는 기다림이 길다", args: vec!["--cwd=C:\\w", "--launch", "--", "claude"],
                      want: expected("C:\\w", None, true, 40.0, &["claude"], false), why: None },
            ArgsRow { note: "--timeout", args: vec!["--cwd", "C:\\w", "--timeout", "5", "--", "claude"],
                      want: expected("C:\\w", None, false, 5.0, &["claude"], false), why: None },
            ArgsRow { note: "상대 경로는 부른 폴더 기준", args: vec!["--cwd", "sub\\dir", "--", "claude"],
                      want: expected("C:\\here\\sub\\dir", None, false, 20.0, &["claude"], false), why: None },
            ArgsRow { note: "-- 뒤의 --cwd 는 claude 인자", args: vec!["--cwd", "C:\\w", "--", "claude", "--cwd", "x", "--"],
                      want: expected("C:\\w", None, false, 20.0, &["claude", "--cwd", "x", "--"], false), why: None },
            ArgsRow { note: "-- 없음", args: vec!["--cwd", "C:\\w", "claude"], want: None, why: Some("모르는 옵션") },
            ArgsRow { note: "-- 로 끝나지 않음", args: vec!["--cwd", "C:\\w"], want: None, why: Some("-- 가 없습니다") },
            ArgsRow { note: "--cwd 없음", args: vec!["--", "claude"], want: None, why: Some("--cwd 가 없습니다") },
            ArgsRow { note: "값 없는 --cwd", args: vec!["--cwd", "--", "claude"], want: None, why: Some("값이 없습니다") },
            ArgsRow { note: "--timeout 범위", args: vec!["--cwd", "C:\\w", "--timeout", "0", "--", "claude"],
                      want: None, why: Some("1~600") },
            ArgsRow { note: "--launch=값", args: vec!["--cwd", "C:\\w", "--launch=1", "--", "claude"],
                      want: None, why: Some("값을 받지 않습니다") },
            ArgsRow { note: "--no-focus 면 선택지 켜짐", args: vec!["--cwd", "C:\\w", "--title", "역할", "--no-focus", "--", "claude"],
                      want: expected("C:\\w", Some("역할"), false, 20.0, &["claude"], true), why: None },
            ArgsRow { note: "--launch --no-focus 함께", args: vec!["--launch", "--no-focus", "--cwd=C:\\w", "--", "claude"],
                      want: expected("C:\\w", None, true, 40.0, &["claude"], true), why: None },
            ArgsRow { note: "--no-focus 가 없으면 꺼짐(기본값)", args: vec!["--cwd", "C:\\w", "--", "claude"],
                      want: expected("C:\\w", None, false, 20.0, &["claude"], false), why: None },
            ArgsRow { note: "-- 뒤의 --no-focus 는 claude 인자", args: vec!["--cwd", "C:\\w", "--", "claude", "--no-focus"],
                      want: expected("C:\\w", None, false, 20.0, &["claude", "--no-focus"], false), why: None },
            ArgsRow { note: "--no-focus=값", args: vec!["--cwd", "C:\\w", "--no-focus=1", "--", "claude"],
                      want: None, why: Some("값을 받지 않습니다") },
        ]
    }

    #[test]
    fn parse_command_line() {
        for row in args_rows() {
            let argv: Vec<String> = row.args.iter().map(|arg| arg.to_string()).collect();
            let got = parse_open_tab_args(&argv, Path::new("C:\\here"));
            match (got, row.want) {
                (Ok(options), Some(want)) => assert_eq!(options, want, "{}", row.note),
                (Ok(_), None) => panic!("{}: 거절해야 한다", row.note),
                (Err(failure), Some(_)) => panic!("{}: 받아야 한다 — {failure:?}", row.note),
                (Err(failure), None) => {
                    let reply = failure.reply();
                    assert_eq!(reply.status, "bad-args", "{}", row.note);
                    let why = reply.why.unwrap_or_default();
                    assert!(why.contains(row.why.unwrap_or("\u{0}")), "{}: why={why}", row.note);
                }
            }
        }
    }

    /// --timeout 경계 — 1·600 은 받고, 0·601·무한·빈 값은 거른다
    #[test]
    fn timeout_seconds_boundaries() {
        let cases: [(&str, bool); 6] = [("1", true), ("600", true), ("0", false), ("600.5", false), ("inf", false), ("", false)];
        for (value, accepted) in cases {
            let argv: Vec<String> = ["--cwd", "C:\\w", "--timeout", value, "--", "claude"].iter().map(|arg| arg.to_string()).collect();
            match parse_open_tab_args(&argv, Path::new("C:\\here")) {
                Ok(options) => assert!(accepted, "--timeout {value}: 거절해야 한다 — {options:?}"),
                Err(failure) => {
                    assert!(!accepted, "--timeout {value}: 받아야 한다 — {failure:?}");
                    assert_eq!(failure.reply().status, "bad-args");
                }
            }
        }
    }

    /// 답은 한 줄 JSON — 키 차례 고정(알파벳), 빗금·한글 그대로(원본 OpenTabTests.swift `replyLine`)
    #[test]
    fn reply_is_one_line_json() {
        let opened = OpenTabReply {
            ok: true,
            pid: None,
            session: Some("0f0e1d2c-3b4a-5968-7766-554433221100".to_string()),
            status: "opened".to_string(),
            tab: Some("역할/2".to_string()),
            why: None,
        };
        assert_eq!(
            opened.line(),
            r#"{"ok":true,"session":"0f0e1d2c-3b4a-5968-7766-554433221100","status":"opened","tab":"역할/2"}"#
        );
        // 실패 답 — ok 가 아니고 why 만 있다
        assert_eq!(OpenTabFailure::BoardOff("꺼짐".to_string()).reply().line(), r#"{"ok":false,"status":"board-off","why":"꺼짐"}"#);
        // exists 답 — pid 가 들어간다
        let exists = OpenTabReply {
            ok: true,
            pid: Some(42),
            session: None,
            status: "exists".to_string(),
            tab: None,
            why: None,
        };
        assert_eq!(exists.line(), r#"{"ok":true,"pid":42,"status":"exists"}"#);
    }

    /// 네 실패가 모두 답 꼴로 돌아가는가 — status 문구가 곧 계약이다(bad-args·board-off·refused·timeout)
    #[test]
    fn failure_reply_status_words() {
        for (failure, status) in [
            (OpenTabFailure::BadArgs("왜".to_string()), "bad-args"),
            (OpenTabFailure::BoardOff("왜".to_string()), "board-off"),
            (OpenTabFailure::Refused("왜".to_string()), "refused"),
            (OpenTabFailure::Timeout("왜".to_string()), "timeout"),
        ] {
            let reply = failure.reply();
            assert_eq!(reply.status, status);
            assert!(!reply.ok);
            assert_eq!(reply.why.as_deref(), Some("왜"));
            assert_eq!(reply.tab, None);
            assert_eq!(reply.session, None);
            assert_eq!(reply.pid, None);
        }
    }

    /// 뒤에서 열기(--no-focus) 요청 칸 — 꺼지면 키가 없다(3.3.3 과 같은 JSON), 켤 때만 true 가 들어간다
    #[test]
    fn request_no_focus_field_is_only_there_when_set() {
        let off = OpenTabRequest {
            id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
            cwd: "C:\\w".to_string(),
            title: Some("역할".to_string()),
            argv: vec!["claude".to_string()],
            at: 1000.0,
            no_focus: None,
        };
        assert_eq!(
            serde_json::to_string(&off).unwrap(),
            r#"{"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"C:\\w","title":"역할","argv":["claude"],"at":1000.0}"#
        );
        assert!(off.reveals());
        let on = OpenTabRequest { no_focus: Some(true), ..off.clone() };
        assert_eq!(
            serde_json::to_string(&on).unwrap(),
            r#"{"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"C:\\w","title":"역할","argv":["claude"],"at":1000.0,"noFocus":true}"#
        );
        assert!(!on.reveals());
        // 칸이 없는 옛 요청 JSON 을 풀면 None(지금 동작 — 앞으로 가져온다)
        let old_json = r#"{"id":"a","cwd":"C:\\w","title":null,"argv":["claude"],"at":0}"#;
        let from_old: OpenTabRequest = serde_json::from_str(old_json).unwrap();
        assert_eq!(from_old.no_focus, None);
        assert!(from_old.reveals());
        // 거짓을 적어 보내도 앞으로 가져온다(켤 때만 true 를 싣는다)
        let false_focus = OpenTabRequest { no_focus: Some(false), ..off.clone() };
        assert!(false_focus.reveals());
    }

    /// 요청 폴더는 설치판·시험 빌드가 따로, 오래된 요청은 받지 않는다(원본 OpenTabTests.swift `placeAndStale` — 경로를 Windows 꼴로 바꿨다)
    #[test]
    fn request_dir_and_stale() {
        assert_eq!(
            open_tab_dir(Path::new("C:\\Local\\AppData"), false),
            PathBuf::from("C:\\Local\\AppData\\claude-board\\open-tab")
        );
        assert_eq!(
            open_tab_dir(Path::new("C:\\Local\\AppData"), true),
            PathBuf::from("C:\\Local\\AppData\\claude-board-dev\\open-tab")
        );
        assert!(!open_tab_stale(1000.0, 1000.0));
        assert!(!open_tab_stale(1000.0, 1600.0));
        assert!(open_tab_stale(1000.0, 1601.0));
        assert!(!open_tab_stale(1060.0, 1000.0));
        assert!(open_tab_stale(1061.0, 1000.0));
    }
}
