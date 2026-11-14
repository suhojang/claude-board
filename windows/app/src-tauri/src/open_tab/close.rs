//! 바깥에서 보드 탭 닫기(ClaudeBoard --close-tab) 규칙 — mac `Sources/BoardRules/CloseTab.swift` 를 그대로 옮긴 것.
//! 바깥 열기(--open-tab)로 연 역할 탭을 그 일이 끝난 뒤 닫는 입구다. 열기와 같은 요청·답 꼴이지만 폴더·알림은 따로다
//! (열기 처리부는 폴더의 `*.req.json` 을 모두 열기 요청으로 읽으므로 섞으면 먹어 버린다).
//! 범위: 바깥 열기로 연 탭(external 표지)이고 화면이 하나인 탭만 닫는다. 사람이 연 탭·나눈 탭·자율주행 이어받기 화면은 거절하고,
//! 보드 탭 밖의 claude 는 찾지도 않는다(신호를 보내지 않는다). 셸 위에서 claude 가 돌면 --stop 일 때만 그 화면에 /exit 를 넣어 끝낸 뒤 닫는다.
//! 파일·시각·프로세스는 앱이 읽어 값으로 넘긴다(이 모듈은 순수 규칙뿐이다).
//!
//! 답은 원본이 `OpenTabReply`(open_tab::args)를 그대로 쓰는데, args 모듈은 다른 청크(C2.2a)가 병행해서 쓰고 아직 이 나무에 없으므로
//! 청크 계약대로 같은 칸(ok·status·tab·session·pid·why)을 지닌 `CloseTabReply` 를 여기 둔다. args 가 들어오면 이 칸을 그쪽으로 바꿔 합친다.

// 모듈 걸기(lib.rs `mod open_tab;`·mod.rs `pub mod close;`)는 통합자 몫 — 걸기 전에는 비공개 모듈 안에서 안 쓰여 경고가 난다
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// MARK: - 명령줄

/// 닫을 대상 — 세션 id(화면의 지금 세션 id) 또는 탭 이름(그 화면의 이름들, 대소문자 무시)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTabTarget {
    /// 세션 id(화면의 지금 세션 id)
    Session(String),
    /// 탭 이름 — 그 화면의 이름들과 대소문자 무시로 같은 화면
    Title(String),
}

/// ClaudeBoard --close-tab 옵션. `stop` 은 claude 가 돌면 /exit 로 끝낸 뒤 닫는다(없으면 busy),
/// `timeout` 은 보드의 답을 기다리는 초
#[derive(Debug, Clone, PartialEq)]
pub struct CloseTabOptions {
    pub target: CloseTabTarget,
    pub stop: bool,
    pub timeout: f64,
}

pub const CLOSE_TAB_USAGE: &str = "쓰는 법: ClaudeBoard --close-tab (--session <세션 id> | --title <탭 이름>) [--stop] [--timeout <초>]";

/// 답을 기다리는 기본 초 — --stop 의 끝나기 기다림(`CLOSE_TAB_STOP_WAIT`)보다 넉넉히
pub const CLOSE_TAB_DEFAULT_TIMEOUT: f64 = 30.0;
/// --stop 으로 /exit 를 넣은 뒤 claude 가 끝나기를 기다리는 상한(초). 넘기면 닫지 않고 busy — 강제 종료는 하지 않는다
pub const CLOSE_TAB_STOP_WAIT: f64 = 10.0;

/// 명령줄 인자 읽기의 실패 — 닫기는 bad-args 한 갈래만 나온다. 원본은 `OpenTabFailure.badArgs` 를 재활용한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTabFailure {
    BadArgs(String),
}

impl CloseTabFailure {
    /// 실패 꼴의 답 — 원본 `OpenTabFailure.reply` 와 같다
    pub fn reply(&self) -> CloseTabReply {
        let CloseTabFailure::BadArgs(why) = self;
        failure_reply("bad-args", why)
    }
}

/// Claude Code 세션 id 꼴(UUID, 8-4-4-4-12 의 16진수)인가 — 원본 OpenTab.swift 의 isSessionID 와 같다.
/// 정규식 의존 없이 자리로 본다: 가운데 자리는 -, 나머지는 16진수 자리
pub fn is_session_id(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let dash_positions = [8usize, 13, 18, 23];
    for (i, b) in s.as_bytes().iter().enumerate() {
        let dash_position = dash_positions.contains(&i);
        if dash_position != (*b == b'-') {
            return false;
        }
        if !dash_position && !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// 글자가 셸에 붙여넣기 안전한가 — 제어 문자(줄바꿈·탭은 됨)는 붙여넣기를 끊고 키로 들어갈 수 있어 거절한다.
/// 원본 OpenTab.swift 의 pasteSafe 와 같다
fn paste_safe(s: &str) -> bool {
    !s.chars().any(|c| {
        let v = c as u32;
        (v < 0x20 && c != '\n' && c != '\t') || v == 0x7f || (0x80..=0x9f).contains(&v) || v == 0x2028 || v == 0x2029
    })
}

/// 탭 이름 꼴인가 — 비지 않은 한 줄, 100자까지, 제어 문자 없음(--open-tab·--close-tab 의 --title 이 함께 쓴다).
/// 원본 OpenTab.swift 의 tabTitleOK 와 같다(원본은 글리프군 수, 이 나무는 유니코드 문자 수 — 잇는 청크에서 다시 본다)
pub fn tab_title_ok(title: &str) -> bool {
    !title.is_empty() && paste_safe(title) && !title.contains('\n') && !title.contains('\t') && title.chars().count() <= 100
}

/// 대상 검사 — 세션 id 와 탭 이름 가운데 하나만. 세션 id 는 UUID 꼴(소문자로 고침), 탭 이름은 --open-tab 의 --title 과 같은 꼴.
/// 명령줄 도구와 보드(요청 파일을 다시 본다)가 함께 쓴다
pub fn close_tab_target(session: Option<&str>, title: Option<&str>) -> Result<CloseTabTarget, CloseTabFailure> {
    match (session, title) {
        (None, None) => Err(CloseTabFailure::BadArgs(format!(
            "--session 이나 --title 가운데 하나가 필요합니다. {CLOSE_TAB_USAGE}"
        ))),
        (Some(_), Some(_)) => Err(CloseTabFailure::BadArgs(format!(
            "--session 과 --title 은 하나만 줍니다. {CLOSE_TAB_USAGE}"
        ))),
        (Some(s), None) => {
            if !is_session_id(s) {
                return Err(CloseTabFailure::BadArgs(format!(
                    "--session 은 세션 id(UUID) 꼴이어야 합니다: {s}"
                )));
            }
            Ok(CloseTabTarget::Session(s.to_lowercase()))
        }
        (None, Some(t)) => {
            if !tab_title_ok(t) {
                return Err(CloseTabFailure::BadArgs("--title 은 한 줄, 100자까지입니다".to_string()));
            }
            Ok(CloseTabTarget::Title(t.to_string()))
        }
    }
}

/// --close-tab 뒤의 인자를 읽는다. 값이 있는 옵션은 다음 인자 또는 --옵션=값(다음 인자가 -- 로 시작하면 값이 없는 것으로 본다 —
/// `--title --stop` 이 "--stop" 이라는 탭을 닫지 않게)
pub fn parse_close_tab_args(args: &[String]) -> Result<CloseTabOptions, CloseTabFailure> {
    let mut session: Option<String> = None;
    let mut title: Option<String> = None;
    let mut stop = false;
    let mut timeout: Option<f64> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        // flag 는 "=" 앞 조각, inline 은 --옵션=값 꼴일 때 "=" 뒤 값 — `--stop=1` 처럼 값 없는 플래그의 = 꼴은 따로 걸러낸다
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if a.starts_with("--") => (f, Some(v)),
            _ => (a.as_str(), None),
        };
        match flag {
            "--session" => session = Some(take_value(args, &mut i, inline, flag)?),
            "--title" => title = Some(take_value(args, &mut i, inline, flag)?),
            "--timeout" => {
                let v = take_value(args, &mut i, inline, flag)?;
                match v.parse::<f64>() {
                    Ok(t) if t.is_finite() && (1.0..=600.0).contains(&t) => timeout = Some(t),
                    _ => return Err(CloseTabFailure::BadArgs(format!("--timeout 은 1~600 초입니다: {v}"))),
                }
            }
            "--stop" => {
                if inline.is_some() {
                    return Err(CloseTabFailure::BadArgs("--stop 은 값을 받지 않습니다".to_string()));
                }
                stop = true;
            }
            _ => {
                return Err(CloseTabFailure::BadArgs(format!("모르는 옵션입니다: {a}. {CLOSE_TAB_USAGE}")));
            }
        }
        i += 1;
    }
    let target = close_tab_target(session.as_deref(), title.as_deref())?;
    Ok(CloseTabOptions {
        target,
        stop,
        timeout: timeout.unwrap_or(CLOSE_TAB_DEFAULT_TIMEOUT),
    })
}

/// 값이 있는 옵션의 값 — --옵션=값 꼴이면 그 값, 아니면 다음 인자(다음이 -- 로 시작하면 값이 없는 것으로 본다)
fn take_value(
    args: &[String],
    i: &mut usize,
    inline: Option<&str>,
    flag: &str,
) -> Result<String, CloseTabFailure> {
    if let Some(v) = inline {
        return Ok(v.to_string()); // = 꼴의 값은 그대로
    }
    match args.get(*i + 1) {
        Some(v) if !v.starts_with("--") => {
            *i += 1;
            Ok(v.clone())
        }
        _ => Err(CloseTabFailure::BadArgs(format!("{flag} 뒤에 값이 없습니다. {CLOSE_TAB_USAGE}"))),
    }
}

// MARK: - 요청·답

/// 요청 파일(<요청 id>.req.json, 닫기 폴더) — 명령줄 도구가 쓰고 보드가 읽은 뒤 지운다. session 과 title 가운데 하나만 있다.
/// 원본은 만든 시각을 1970 기준 초(Double)로 싣는다 — 정수인 초는 소수점 없이 압다(원본 JSONEncoder 와 같게)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CloseTabRequest {
    #[serde(with = "epoch_seconds")]
    pub at: f64,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub stop: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

mod epoch_seconds {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// 정수인 초는 정수로(원본 JSONEncoder 출력과 같게), 나머지는 실수로
    pub fn serialize<S: Serializer>(at: &f64, s: S) -> Result<S::Ok, S::Error> {
        if at.fract() == 0.0 && (i64::MIN as f64..=i64::MAX as f64).contains(at) {
            s.serialize_i64(*at as i64)
        } else {
            match serde_json::Number::from_f64(*at) {
                Some(n) => n.serialize(s),
                None => Err(serde::ser::Error::custom(String::from("숫자로 압을 수 없는 at"))),
            }
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        f64::deserialize(d)
    }
}

impl CloseTabRequest {
    /// target 에서 session·title 가운데 하나만 싣는다(원본 생성자와 같다)
    pub fn new(id: impl Into<String>, target: &CloseTabTarget, stop: bool, at: f64) -> CloseTabRequest {
        let (session, title) = match target {
            CloseTabTarget::Session(s) => (Some(s.clone()), None),
            CloseTabTarget::Title(t) => (None, Some(t.clone())),
        };
        CloseTabRequest { at, id: id.into(), session, stop, title }
    }
}

/// 답(<요청 id>.reply.json, 명령줄 도구가 한 줄 JSON 으로 그대로 찍는다). status 는 closed(닫음, --stop 이면 claude 를 끝낸 뒤)·
/// not-found·busy·refused·bad-args·board-off·timeout. 원본은 `OpenTabReply` 를 그대로 쓰는데 이 청크 나무에 args 모듈이 아직
/// 없으므로 같은 칸을 지닌 짝으로 두었다(머리 문서 참고).
/// 칸은 한 줄 JSON 키 차례가 고정되도록(원본의 sortedKeys 와 같게) 자모 순으로 두었다: ok·pid·session·status·tab·why
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CloseTabReply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl CloseTabReply {
    /// 한 줄 JSON — 키 차례 고정, 빗금·한글 그대로(원본 `OpenTabReply.line` 과 같다)
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from(r#"{"ok":false,"status":"error"}"#))
    }
}

/// 실패 꼴 답의 어깨 — ok false, 상태·이유만. 칸 순서는 struct 선언(자모 순)을 따른다
fn failure_reply_skeleton() -> CloseTabReply {
    CloseTabReply {
        ok: false,
        pid: None,
        session: None,
        status: String::new(),
        tab: None,
        why: None,
    }
}

/// 실패 꼴 답 — 상태와 이유만 있다
fn failure_reply(status: &str, why: &str) -> CloseTabReply {
    CloseTabReply {
        status: status.to_string(),
        why: Some(why.to_string()),
        ..failure_reply_skeleton()
    }
}

/// 요청 폴더 — 열기 폴더(open-tab) 옆의 close-tab. 설치판·시험 빌드가 따로다
pub fn close_tab_dir(bundle_id: Option<&str>, home: &str) -> String {
    let app_name = if bundle_id == Some(BOARD_DEV_BUNDLE_ID) { "Claude Board Dev" } else { "Claude Board" };
    format!("{home}/Library/Application Support/{app_name}/close-tab")
}

/// 시험 빌드 앱의 bundle id(원본은 BoardLog.swift 의 `boardDevBundleID` 를 쓴다)
pub const BOARD_DEV_BUNDLE_ID: &str = "com.claude.board.dev";

/// 닫기 요청 하나의 상한 — 세션 id 나 100자 탭 이름이라 작다
pub const CLOSE_TAB_MAX_BYTES: usize = 16 * 1024;

// MARK: - 판정

/// 보드 탭 화면 하나의 지금 모양(앞 프로그램)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTabForeground {
    /// 셸이 아직 뜨는 중이거나 보드가 claude 명령을 막 넣음(셸이 뜨기를 기다림·넣은 지 3초 안)
    Starting,
    /// 셸 프롬프트(셸 위에 도는 프로그램이 없음)
    Prompt,
    /// 앞 프로그램이 살아 있는 claude
    Claude,
    /// 셸에서 띄운 다른 프로그램(앞 프로그램을 읽지 못한 때도)
    Other,
}

/// 닫기 판정에 넘길 화면 값 — 앱이 BoardSurface·BoardTab 에서 떠 넘긴다
#[derive(Debug, Clone, PartialEq)]
pub struct CloseTabScreen {
    /// 화면 id(탭 화면 하나의 id, 답의 tab·session 을 되찾는 열쇠) — 원본은 UUID 값, 이 나무는 id 를 문자열로 쥔다
    pub id: String,
    /// 탭 이름(답의 tab)
    pub tab: String,
    /// 화면의 지금 세션 id(짝지은 세션, 없으면 화면이 기억한 세션 id)
    pub session_id: Option<String>,
    /// 바깥 열기의 이름 찾기와 같은 이름들(화면 하나인 탭만 탭 이름이 든다)
    pub names: Vec<String>,
    /// 바깥 열기로 연 탭(external 표지)
    pub external: bool,
    /// 자율주행을 이어받은 화면
    pub takeover: bool,
    /// 탭의 화면이 둘 이상(사람이 나눈 탭)
    pub split: bool,
    /// 앞 닫기 요청(--stop)이 이 화면의 claude 를 끝내는 중
    pub stopping: bool,
    pub foreground: CloseTabForeground,
}

/// 판정 결과 — 닫기(close·stopThenClose)는 앱이 닫은 뒤 closed 로 답한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTabPlan {
    /// 셸 프롬프트 — 바로 닫는다
    Close(String),
    /// --stop: 그 화면의 claude 에 /exit 를 넣고 끝나면 닫는다
    StopThenClose(String),
    NotFound(String),
    /// 탭은 그대로
    Busy(String, String),
    /// 범위 밖 — 아무것도 닫지 않는다(여러 탭이 맞으면 id 없음)
    Refuse(Option<String>, String),
}

/// 대상과 맞는 화면을 찾아 판정한다 — 세션 id 는 화면의 세션 id 와 같은 화면, 이름은 화면의 이름들과 대소문자 무시로 같은 화면.
/// 없으면 not-found, 둘 이상이면 거절(아무것도 닫지 않음), 하나면 `close_tab_decide`
pub fn plan_close_tab(target: &CloseTabTarget, stop: bool, screens: &[CloseTabScreen]) -> CloseTabPlan {
    let matched: Vec<&CloseTabScreen> = match target {
        CloseTabTarget::Session(id) => {
            let key = id.to_lowercase();
            screens
                .iter()
                .filter(|s| s.session_id.as_deref().is_some_and(|v| v.to_lowercase() == key))
                .collect()
        }
        CloseTabTarget::Title(t) => {
            let key = t.to_lowercase();
            screens
                .iter()
                .filter(|s| s.names.iter().any(|n| n.to_lowercase() == key))
                .collect()
        }
    };
    let Some(first) = matched.first() else {
        return match target {
            CloseTabTarget::Session(id) => CloseTabPlan::NotFound(format!(
                "이 세션의 보드 탭이 없습니다(이미 닫혔을 수 있습니다): {id}"
            )),
            CloseTabTarget::Title(t) => CloseTabPlan::NotFound(format!(
                "이 이름의 보드 탭이 없습니다(이미 닫혔을 수 있습니다): {t}"
            )),
        };
    };
    if matched.len() != 1 {
        let ids = matched
            .iter()
            .map(|s| s.session_id.clone().unwrap_or_else(|| String::from("모름")))
            .collect::<Vec<_>>()
            .join(", ");
        let count = matched.len();
        return CloseTabPlan::Refuse(
            None,
            format!("여러 탭이 맞습니다({count}개, 세션 {ids}). 아무것도 닫지 않았습니다. --session 으로 하나를 고르세요"),
        );
    }
    close_tab_decide(first, stop)
}

/// 맞는 화면 하나의 판정 — 이어받기·표지 없음·나눈 탭은 거절, 끝내는 중·막 띄우는 중·다른 프로그램은 busy,
/// 셸 프롬프트면 닫기, claude 면 --stop 일 때만 끝낸 뒤 닫기(없으면 busy). --stop 뒤 다시 볼 때도 부른다(stop false)
pub fn close_tab_decide(s: &CloseTabScreen, stop: bool) -> CloseTabPlan {
    if s.takeover {
        return CloseTabPlan::Refuse(Some(s.id.clone()), "자율주행을 이어받은 탭은 닫지 않습니다".to_string());
    }
    if !s.external {
        return CloseTabPlan::Refuse(
            Some(s.id.clone()),
            "바깥 열기(--open-tab)로 연 탭이 아니라 닫지 않습니다(사람이 연 탭, 또는 이 기능 전 판에서 연 탭은 손으로 닫습니다)".to_string(),
        );
    }
    if s.split {
        return CloseTabPlan::Refuse(Some(s.id.clone()), "화면을 나눈 탭은 닫지 않습니다".to_string());
    }
    if s.stopping {
        return CloseTabPlan::Busy(s.id.clone(), "이미 이 탭의 claude 를 끝내는 중입니다".to_string());
    }
    match s.foreground {
        CloseTabForeground::Starting => CloseTabPlan::Busy(
            s.id.clone(),
            "탭이 막 뜨는 중입니다(셸이 뜨는 중이거나 claude 명령을 막 넣음). 잠시 뒤 다시 하세요".to_string(),
        ),
        CloseTabForeground::Prompt => CloseTabPlan::Close(s.id.clone()),
        CloseTabForeground::Claude => {
            if stop {
                CloseTabPlan::StopThenClose(s.id.clone())
            } else {
                CloseTabPlan::Busy(s.id.clone(), "claude 가 돌고 있습니다. --stop 을 주면 끝낸 뒤 닫습니다".to_string())
            }
        }
        CloseTabForeground::Other => {
            CloseTabPlan::Busy(s.id.clone(), "셸에서 다른 프로그램이 돌고 있어 닫지 않습니다".to_string())
        }
    }
}

/// 판정 결과의 답 — 닫기(close·stopThenClose)는 앱이 닫은 뒤 closed 로 답한다(None). 맞은 탭의 이름·세션을 싣는다(여러 탭·없음은 빼고)
pub fn close_tab_reply(plan: &CloseTabPlan, screens: &[CloseTabScreen]) -> Option<CloseTabReply> {
    // 답 꼴의 어깨를 붙잡아 맞은 화면의 이름·세션을 되찾는다(원본 plan 꼴과 같다: busy 의 id 는 받음, refuse 는 여러 탭이면 없음)
    let (status, id, why): (&str, Option<&str>, &str) = match plan {
        CloseTabPlan::Close(_) | CloseTabPlan::StopThenClose(_) => return None,
        CloseTabPlan::NotFound(why) => ("not-found", None, why),
        CloseTabPlan::Busy(id, why) => ("busy", Some(id), why),
        CloseTabPlan::Refuse(id, why) => ("refused", id.as_deref(), why),
    };
    let (tab, session) = id
        .and_then(|id| screens.iter().find(|s| s.id == id))
        .map(|s| (Some(s.tab.clone()), s.session_id.clone()))
        .unwrap_or((None, None));
    Some(CloseTabReply {
        ok: false,
        pid: None,
        session,
        status: status.to_string(),
        tab,
        why: Some(why.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 원본 CloseTabTests 의 자리표
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";
    const SID2: &str = "11111111-2222-3333-4444-555555555555";
    const A: &str = "00000000-0000-0000-0000-00000000000a";
    const B: &str = "00000000-0000-0000-0000-00000000000b";

    /// 기본 화면 — tab "역할", external true, fg 셸 프롬프트(원본 `screen(_:)` 기본값과 같다)
    fn base_screen(id: &str) -> CloseTabScreen {
        CloseTabScreen {
            id: id.to_string(),
            tab: "역할".to_string(),
            session_id: None,
            names: Vec::new(),
            external: true,
            takeover: false,
            split: false,
            stopping: false,
            foreground: CloseTabForeground::Prompt,
        }
    }

    /// 화면 값 한 칸을 바꾼 사본 — 원본 screen 헬퍼의 인자 자리다
    fn screen_with(id: &str, set: impl FnOnce(&mut CloseTabScreen)) -> CloseTabScreen {
        let mut s = base_screen(id);
        set(&mut s);
        s
    }

    fn opts(target: CloseTabTarget, stop: bool, timeout: f64) -> CloseTabOptions {
        CloseTabOptions { target, stop, timeout }
    }

    fn svec(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// ① 표의 한 줄 — (이름, 인자, 기다린 받음, 실패면 why 에 든 글자)
    type ArgsRow = (&'static str, Vec<String>, Option<CloseTabOptions>, Option<&'static str>);

    /// ① 명령줄 읽기 — 원본 표(ArgsRow) 한 줄씩. 실패 줄은 bad-args 답의 why 에 기다린 글자가 든다
    #[test]
    fn args_table() {
        let title_101 = "가".repeat(101);
        let rows: Vec<ArgsRow> = vec![
            ("세션 id", svec(&["--session", SID]), Some(opts(CloseTabTarget::Session(SID.into()), false, 30.0)), None),
            ("대문자 세션 id 는 소문자로", svec(&["--session", &SID.to_uppercase()]),
                Some(opts(CloseTabTarget::Session(SID.into()), false, 30.0)), None),
            ("탭 이름", svec(&["--title", "역할 2"]), Some(opts(CloseTabTarget::Title("역할 2".into()), false, 30.0)), None),
            ("= 꼴과 --stop", svec(&[&format!("--session={SID}"), "--stop"]),
                Some(opts(CloseTabTarget::Session(SID.into()), true, 30.0)), None),
            ("--title= 꼴", svec(&["--stop", "--title=exp-close-1"]),
                Some(opts(CloseTabTarget::Title("exp-close-1".into()), true, 30.0)), None),
            ("--timeout", svec(&["--title", "a", "--timeout", "600"]),
                Some(opts(CloseTabTarget::Title("a".into()), false, 600.0)), None),
            ("--timeout= 꼴", svec(&["--title", "a", "--timeout=1"]),
                Some(opts(CloseTabTarget::Title("a".into()), false, 1.0)), None),
            ("대상 없음", Vec::new(), None, Some("하나가 필요합니다")),
            ("--stop 만", svec(&["--stop"]), None, Some("하나가 필요합니다")),
            ("둘 다 있음", svec(&["--session", SID, "--title", "a"]), None, Some("하나만")),
            ("세션 id 꼴이 아님", svec(&["--session", "역할"]), None, Some("UUID")),
            ("모르는 옵션", svec(&["--title", "a", "--launch"]), None, Some("모르는 옵션")),
            ("자리 인자는 받지 않음", svec(&["--title", "a", "b"]), None, Some("모르는 옵션")),
            ("-- 도 받지 않음", svec(&["--title", "a", "--"]), None, Some("모르는 옵션")),
            ("값 없는 --title", svec(&["--title"]), None, Some("값이 없습니다")),
            ("다음 인자가 옵션이면 값 없음", svec(&["--title", "--stop"]), None, Some("값이 없습니다")),
            ("값 없는 --session", svec(&["--session"]), None, Some("값이 없습니다")),
            ("값 없는 --timeout", svec(&["--title", "a", "--timeout"]), None, Some("값이 없습니다")),
            ("--timeout 0", svec(&["--title", "a", "--timeout", "0"]), None, Some("1~600")),
            ("--timeout 601", svec(&["--title", "a", "--timeout", "601"]), None, Some("1~600")),
            ("--timeout 숫자 아님", svec(&["--title", "a", "--timeout", "x"]), None, Some("1~600")),
            ("--stop=값", svec(&["--title", "a", "--stop=1"]), None, Some("값을 받지 않습니다")),
            ("여러 줄 탭 이름", svec(&["--title", "가\n나"]), None, Some("한 줄")),
            ("빈 탭 이름", svec(&["--title="]), None, Some("한 줄")),
            ("101자 탭 이름", vec!["--title".to_string(), title_101], None, Some("100자")),
            ("제어 문자 탭 이름", svec(&["--title", "a\u{1b}b"]), None, Some("한 줄")),
        ];
        assert_eq!(rows.len(), 26, "원본 표의 줄 수");
        for (note, args, want, why) in &rows {
            match parse_close_tab_args(args) {
                Ok(o) => {
                    assert!(want.is_some(), "{note}: 실패를 기다렸다");
                    assert_eq!(want.as_ref(), Some(&o), "{note}");
                }
                Err(e) => {
                    let reply = e.reply();
                    assert!(want.is_none(), "{note}: 받음을 기다렸다");
                    assert_eq!(reply.status, "bad-args", "{note}");
                    assert!(reply.why.as_deref().unwrap_or_default().contains(why.unwrap_or_default()), "{note}: {:?}", reply.why);
                }
            }
        }
    }

    /// 판정 표의 기다림(원본 `Want`)
    #[derive(Debug, PartialEq)]
    enum Want<'a> {
        Close(&'a str),
        StopThenClose(&'a str),
        /// 답의 status 와 why 에 든 글자
        Status(&'a str, &'a str),
    }

    /// ② 대상 찾기와 판정 — 원본 표(PlanRow) 한 줄씩
    #[test]
    fn plan_table() {
        let rows: Vec<(&str, CloseTabTarget, bool, Vec<CloseTabScreen>, Want)> = vec![
            ("세션 id 로 맞음 → 닫기", CloseTabTarget::Session(SID.into()), false,
                vec![screen_with(A, |s| s.session_id = Some(SID2.into())), screen_with(B, |s| s.session_id = Some(SID.into()))],
                Want::Close(B)),
            ("화면 세션 id 가 대문자여도 맞음", CloseTabTarget::Session(SID.into()), false,
                vec![screen_with(A, |s| s.session_id = Some(SID.to_uppercase()))], Want::Close(A)),
            ("이름(대소문자 무시)으로 맞음 → 닫기", CloseTabTarget::Title("Exp-Close-1".into()), false,
                vec![screen_with(A, |s| s.names = vec!["exp-close-2".into()]), screen_with(B, |s| s.names = vec!["exp-close-1".into()])],
                Want::Close(B)),
            ("셸만 남은 화면이 탭 이름으로만 맞음 → 닫기", CloseTabTarget::Title("exp-close-1".into()), false,
                vec![screen_with(A, |s| { s.tab = "exp-close-1".into(); s.session_id = Some(SID.into()); s.names = vec!["exp-close-1".into()]; })],
                Want::Close(A)),
            ("나눈 탭의 화면은 탭 이름으로 맞지 않음(이름 목록에 탭 이름이 없음) → not-found", CloseTabTarget::Title("역할".into()), false,
                vec![screen_with(A, |s| s.split = true), screen_with(B, |s| { s.split = true; s.names = vec!["다른 세션".into()]; })],
                Want::Status("not-found", "이 이름")),
            ("맞는 것 없음(세션) → not-found", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| s.session_id = Some(SID2.into()))], Want::Status("not-found", SID)),
            ("화면이 없음 → not-found", CloseTabTarget::Title("역할".into()), false, Vec::new(), Want::Status("not-found", "역할")),
            ("둘 이상 맞음 → refused(아무것도 닫지 않음)", CloseTabTarget::Title("exp-close-5".into()), false,
                vec![screen_with(A, |s| s.names = vec!["exp-close-5".into()]), screen_with(B, |s| s.names = vec!["EXP-CLOSE-5".into()])],
                Want::Status("refused", "여러 탭")),
            ("둘 이상 맞으면 하나가 표지 없는 탭이어도 refused(여러 탭)", CloseTabTarget::Title("x".into()), true,
                vec![screen_with(A, |s| { s.names = vec!["x".into()]; s.external = false; }), screen_with(B, |s| { s.names = vec!["x".into()]; s.foreground = CloseTabForeground::Claude; })],
                Want::Status("refused", "여러 탭")),
            ("표지 없는 탭 → refused", CloseTabTarget::Title("exp-close-3".into()), false,
                vec![screen_with(A, |s| { s.names = vec!["exp-close-3".into()]; s.external = false; })], Want::Status("refused", "바깥 열기")),
            ("표지 없는 탭은 --stop 이어도 claude 를 끝내지 않음", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.external = false; s.foreground = CloseTabForeground::Claude; })],
                Want::Status("refused", "바깥 열기")),
            ("이어받기 화면 → refused", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.takeover = true; })], Want::Status("refused", "이어받은")),
            ("이어받기가 표지보다 먼저", CloseTabTarget::Session(SID.into()), false,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.external = false; s.takeover = true; })],
                Want::Status("refused", "이어받은")),
            ("나눈 탭 → refused", CloseTabTarget::Session(SID.into()), false,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.split = true; }), screen_with(B, |s| s.split = true)],
                Want::Status("refused", "나눈 탭")),
            ("나눈 탭은 --stop 이어도 refused", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.split = true; s.foreground = CloseTabForeground::Claude; })],
                Want::Status("refused", "나눈 탭")),
            ("셸이 뜨는 중·막 넣음 → busy", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.foreground = CloseTabForeground::Starting; })],
                Want::Status("busy", "막 뜨는 중")),
            ("claude 도는 중(stop 없음) → busy", CloseTabTarget::Session(SID.into()), false,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.foreground = CloseTabForeground::Claude; })],
                Want::Status("busy", "--stop")),
            ("claude 도는 중(stop 있음) → 끝낸 뒤 닫기", CloseTabTarget::Title("역할".into()), true,
                vec![screen_with(A, |s| { s.names = vec!["역할".into()]; s.foreground = CloseTabForeground::Claude; })],
                Want::StopThenClose(A)),
            ("다른 프로그램 도는 중(stop 있어도) → busy", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.foreground = CloseTabForeground::Other; })],
                Want::Status("busy", "다른 프로그램")),
            ("셸 프롬프트는 --stop 이어도 바로 닫기", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| s.session_id = Some(SID.into()))], Want::Close(A)),
            ("이미 끝내는 중인 화면 → busy", CloseTabTarget::Session(SID.into()), true,
                vec![screen_with(A, |s| { s.session_id = Some(SID.into()); s.stopping = true; s.foreground = CloseTabForeground::Claude; })],
                Want::Status("busy", "끝내는 중")),
        ];
        assert_eq!(rows.len(), 21, "원본 표의 줄 수");
        for (note, target, stop, screens, want) in &rows {
            let plan = plan_close_tab(target, *stop, screens);
            match want {
                Want::Close(id) => {
                    assert_eq!(plan, CloseTabPlan::Close((*id).to_string()), "{note}");
                    assert!(close_tab_reply(&plan, screens).is_none(), "{note}");
                }
                Want::StopThenClose(id) => {
                    assert_eq!(plan, CloseTabPlan::StopThenClose((*id).to_string()), "{note}");
                    assert!(close_tab_reply(&plan, screens).is_none(), "{note}");
                }
                Want::Status(status, why) => {
                    let reply = close_tab_reply(&plan, screens);
                    let reply = reply.unwrap_or_else(|| panic!("{note}: 답이 없다"));
                    assert!(!reply.ok, "{note}");
                    assert_eq!(reply.status, *status, "{note}");
                    assert!(
                        reply.why.as_deref().unwrap_or_default().contains(why),
                        "{note}: 기다린 글자 {why:?}, 든 이유 {:?}",
                        reply.why
                    );
                }
            }
        }
    }

    /// ③ 답에는 맞은 탭의 이름·세션을 싣는다(여러 탭·없음은 빼고)
    #[test]
    fn reply_info() {
        let s = screen_with(A, |s| { s.tab = "exp-close-1".into(); s.session_id = Some(SID.into()); s.foreground = CloseTabForeground::Claude; });
        let one = std::slice::from_ref(&s);
        let busy = close_tab_reply(&plan_close_tab(&CloseTabTarget::Session(SID.into()), false, one), one).unwrap();
        assert_eq!(
            busy.line(),
            format!(r#"{{"ok":false,"session":"{SID}","status":"busy","tab":"exp-close-1","why":"claude 가 돌고 있습니다. --stop 을 주면 끝낸 뒤 닫습니다"}}"#)
        );
        let two = vec![base_screen(A), base_screen(B)]; // 둘 다 이름 "역할"
        let many = close_tab_reply(&plan_close_tab(&CloseTabTarget::Title("역할".into()), false, &two), &two).unwrap();
        assert!(many.tab.is_none() && many.session.is_none());
        // --stop 뒤 다시 볼 때(셸 프롬프트로 돌아옴)
        assert_eq!(close_tab_decide(&base_screen(A), false), CloseTabPlan::Close(A.into()));
        assert_eq!(
            close_tab_decide(&screen_with(A, |s| s.foreground = CloseTabForeground::Claude), false),
            CloseTabPlan::Busy(A.into(), "claude 가 돌고 있습니다. --stop 을 주면 끝낸 뒤 닫습니다".to_string())
        );
    }

    /// ④ 답은 한 줄 JSON — 키 차례 고정, 빗금·한글 그대로
    #[test]
    fn reply_line() {
        let r = CloseTabReply {
            ok: true,
            status: "closed".to_string(),
            tab: Some("역할/2".to_string()),
            session: Some(SID.to_string()),
            ..failure_reply_skeleton()
        };
        assert_eq!(
            r.line(),
            format!(r#"{{"ok":true,"session":"{SID}","status":"closed","tab":"역할/2"}}"#)
        );
        assert!(!r.line().contains('\n'));
        let not_found = CloseTabReply {
            status: "not-found".to_string(),
            why: Some("없음/닫힘".to_string()),
            ..failure_reply_skeleton()
        };
        assert_eq!(not_found.line(), r#"{"ok":false,"status":"not-found","why":"없음/닫힘"}"#);
    }

    /// ⑤ 요청 폴더는 설치판·시험 빌드가 따로, 열기 폴더와 다른 자리
    #[test]
    fn place() {
        assert_eq!(close_tab_dir(Some("com.claude.board"), "/Users/a"), "/Users/a/Library/Application Support/Claude Board/close-tab");
        assert_eq!(close_tab_dir(None, "/Users/a"), "/Users/a/Library/Application Support/Claude Board/close-tab");
        assert_eq!(close_tab_dir(Some(BOARD_DEV_BUNDLE_ID), "/Users/a"), "/Users/a/Library/Application Support/Claude Board Dev/close-tab");
        // 열기 폴더(open_tab::args 의 open_tab_dir)와 같은 자리가 아니어야 한다 — args 모듈이 아직 이 나무에 없어 원본 OpenTab.swift 의 꼴을 그대로 썼다
        let open_tab_dir = |bundle_id: Option<&str>, home: &str| {
            let app = if bundle_id == Some(BOARD_DEV_BUNDLE_ID) { "Claude Board Dev" } else { "Claude Board" };
            format!("{home}/Library/Application Support/{app}/open-tab")
        };
        for id in ["com.claude.board", "com.claude.board.dev"] {
            assert_ne!(close_tab_dir(Some(id), "/Users/a"), open_tab_dir(Some(id), "/Users/a"));
        }
    }

    /// ⑥ 요청 꼴 — 대상은 session·title 가운데 하나만 싣고, 다시 풀면 같다
    #[test]
    fn request() {
        let by_session = CloseTabRequest::new(SID2, &CloseTabTarget::Session(SID.into()), true, 1000.0);
        assert_eq!(
            serde_json::to_string(&by_session).unwrap(),
            format!(r#"{{"at":1000,"id":"{SID2}","session":"{SID}","stop":true}}"#)
        );
        let by_title = CloseTabRequest::new(SID2, &CloseTabTarget::Title("역할/2".into()), false, 1000.0);
        assert_eq!(
            serde_json::to_string(&by_title).unwrap(),
            format!(r#"{{"at":1000,"id":"{SID2}","stop":false,"title":"역할/2"}}"#)
        );
        let by_title_json = serde_json::to_string(&by_title).unwrap();
        assert_eq!(serde_json::from_str::<CloseTabRequest>(&by_title_json).unwrap(), by_title);
        // 보드는 요청 파일의 대상을 다시 본다 — 둘 다·둘 다 없음·꼴이 틀린 세션은 bad-args
        assert_eq!(close_tab_target(Some(SID), None).unwrap(), CloseTabTarget::Session(SID.into()));
        assert_eq!(close_tab_target(None, Some("역할")).unwrap(), CloseTabTarget::Title("역할".into()));
        for (s, t) in [(Some(SID), Some("역할")), (None, None), (Some("x"), None)] {
            let f = close_tab_target(s, t).expect_err("기다린 실패");
            assert_eq!(f.reply().status, "bad-args");
        }
    }
}

