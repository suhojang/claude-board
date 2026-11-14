//! 바깥에서 보드 탭 열기(`--open-tab`)의 규칙 — 요청 검사를 띄울 꼴로 고치기(`open_tab_prepare`), claude 인자 읽기(`claude_arg_info`),
//! 이미 있는 탭 가리기(`plan_open_tab`), 답변 꾸미기(md-render)를 인자 목록에서 고치는 `open_tab_command`.
//! mac `Sources/BoardRules/OpenTab.swift` 140-312 를 옮긴다(OpenTabTests.swift 의 prepare·plan·command 시험도 같이 옮겼다).
//!
//! mac 과 다른 점 두 가지(Windows 판 결정):
//! - mac 은 인자마다 따옴표로 묶어 셸에 치고 다시 읽어 같은 인자인지 확인하고, Windows 판은 셸 문자열을 만들지 않는다
//!   — 명령(`open_tab_command`)은 낱말 argv 로 넘기고, 붙이기·실행을 낱말 단위로 하는 쪽에서 묶는다. 그래서 `OpenTabPlanInput` 에
//!   `command` 칸이 없고, 읽지 못하는 명령의 `OpenTabFailure::Refused` 갈래도 없다.
//! - mac 은 `cwd` 가 `/` 로 시작하는지 보지만, Windows 판은 드라이브·UNC 절대 경로(std `Path::is_absolute`)를 본다.
//!
//! 파일·시각·프로세스는 그 밖(app)의 어댑터가 읽어 값으로 넘긴다 — 이 모듈은 순수 규칙이다.

use std::collections::HashSet;

use super::args::OpenTabFailure;

/// 요청 하나의 상한 — 첫 요청 글이 길어도 넉넉하게, 셸에 붙여넣을 수 있는 만큼
pub const OPEN_TAB_MAX_BYTES: usize = 256 * 1024;

/// 요청을 검사하고 띄울 꼴로 고친다. `new_id` 는 새 세션에 붙일 id(앱이 UUID 로 만들어 넘긴다).
/// mac `OpenTabPrepareResult`(OpenTab.swift 214-248)의 argv·sessionId·resumes·name 그대로다 —
/// Windows 판은 명령을 낱말로 넘기므로 mac 의 `command`(셸 문자열) 칸 대신 요청 cwd 를 싣는다
/// (`plan_open_tab` 이 brief 의 서명대로 input 하나만 받아 화면과 폴더를 견주려면 필요하다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTabPlanInput {
    /// 새 세션이면 claude 바로 뒤에 --session-id <새 id> 를 넣은 인자
    pub argv: Vec<String>,
    /// 이 탭의 세션 id(이어 여는 세션·--session-id·새로 붙인 id), 모르면 None
    pub session_id: Option<String>,
    /// session_id 가 --resume 으로 이어 여는 세션이다(대화 기록이 있어야 뜬다)
    pub resumes: bool,
    /// 탭 이름이자 세션 이름(--name 이 없으면 --title)
    pub name: Option<String>,
    /// 탭을 열 폴더(mac 에선 planOpenTab 이 따로 받지만, Windows 판은 여기 싣는다)
    pub cwd: String,
}

/// claude 인자에서 읽은 것
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClaudeArgInfo {
    /// --name·-n
    pub name: Option<String>,
    /// --resume·-r 의 값(없이 주면 고르기 — None 이고 picker)
    pub resume: Option<String>,
    /// 값 없는 --resume·-r
    pub picker: bool,
    /// --continue·-c
    pub continues: bool,
    /// --session-id
    pub session_flag: Option<String>,
    /// --fork-session
    pub fork: bool,
}

/// 다음 인자를 값으로 — inline(--옵션=값)이 있으면 그것, 아니면 다음 인자(- 로 시작하지 않을 때)를 먹는다(먹었으면 true)
fn next_value<'a>(argv: &'a [String], i: usize, inline: Option<&'a str>) -> (Option<&'a str>, bool) {
    if let Some(v) = inline {
        return (Some(v), false);
    }
    if i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
        (Some(argv[i + 1].as_str()), true)
    } else {
        (None, false)
    }
}

/// claude 인자(첫 인자 claude 뒤)를 훑어 이름·이어 열기·세션 id 를 읽는다. 값은 다음 인자(-로 시작하지 않을 때) 또는 --옵션=값
pub fn claude_arg_info(argv: &[String]) -> ClaudeArgInfo {
    let mut info = ClaudeArgInfo::default();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        // mac 은 -- 로 시작하고 = 을 담은 인자만 나눈다 — -n=가 같은 짧은 꼴은 낱말째 하나다
        let (flag, inline): (&str, Option<&str>) = if a.starts_with("--") {
            match a.split_once('=') {
                Some((f, v)) => (f, Some(v)),
                None => (a.as_str(), None),
            }
        } else {
            (a.as_str(), None)
        };
        match flag {
            "--name" | "-n" => {
                if let (Some(v), ate) = next_value(argv, i, inline) {
                    info.name = Some(v.to_owned());
                    if ate {
                        i += 1;
                    }
                }
            }
            "--resume" | "-r" => {
                let (v, ate) = next_value(argv, i, inline);
                match v {
                    Some(v) => {
                        info.resume = Some(v.to_owned());
                        info.picker = false;
                    }
                    None => info.picker = true,
                }
                if ate {
                    i += 1;
                }
            }
            "--continue" | "-c" => info.continues = true,
            "--session-id" => {
                if let (Some(v), ate) = next_value(argv, i, inline) {
                    info.session_flag = Some(v.to_owned());
                    if ate {
                        i += 1;
                    }
                }
            }
            "--fork-session" => info.fork = true,
            _ => {}
        }
        i += 1;
    }
    info
}

/// Claude Code 세션 id 꼴(UUID)인가 — 8-4-4-4-12 자리의 16진수
pub fn is_session_id(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    const LENGTHS: [usize; 5] = [8, 4, 4, 4, 12];
    groups.len() == 5
        && groups.iter().zip(LENGTHS).all(|(g, len)| {
            g.len() == len && g.chars().all(|c| c.is_ascii_hexdigit())
        })
}

/// 글자가 셸에 붙여넣기 안전한가 — 제어 문자(줄바꿈·탭은 됨)는 붙여넣기를 끊고 키로 들어갈 수 있어 거절한다
fn paste_safe(s: &str) -> bool {
    !s.chars().any(|c| {
        let v = c as u32;
        (v < 0x20 && c != '\n' && c != '\t')
            || v == 0x7F
            || (0x80..=0x9F).contains(&v)
            || v == 0x2028
            || v == 0x2029
    })
}

/// 탭 이름 꼴인가 — 비지 않은 한 줄, 100자까지, 제어 문자 없음(--open-tab·--close-tab 의 --title 이 함께 쓴다)
pub fn tab_title_ok(title: &str) -> bool {
    !title.is_empty()
        && paste_safe(title)
        && !title.contains('\n')
        && !title.contains('\t')
        && title.chars().count() <= 100
}

/// `--open-tab` 요청을 검사해 띄울 꼴로 고친다. mac `openTabPrepare`(OpenTab.swift 281-312)과 실패 문구를 그대로 옮겼다.
/// 차이 — cwd 는 Windows 절대 경로로 본다. 새 세션(claude 의 인자에 이어 열기·세션 id 가 없을 때)은
/// claude 바로 뒤에 `--session-id <new_id>` 를 넣는다.
pub fn open_tab_prepare(cwd: &str, title: Option<&str>, argv: &[String], new_id: &str) -> Result<OpenTabPlanInput, OpenTabFailure> {
    if !std::path::Path::new(cwd).is_absolute() || !paste_safe(cwd) || cwd.contains('\n') || cwd.contains('\t') {
        return Err(OpenTabFailure::BadArgs(format!("--cwd 는 절대 경로여야 합니다: {cwd}")));
    }
    let Some(first) = argv.first() else {
        return Err(OpenTabFailure::BadArgs("claude 명령만 띄웁니다(-- 뒤 첫 인자가 claude 여야 함): (없음)".to_string()));
    };
    if first != "claude" {
        return Err(OpenTabFailure::BadArgs(format!("claude 명령만 띄웁니다(-- 뒤 첫 인자가 claude 여야 함): {first}")));
    }
    if !argv.iter().all(|a| paste_safe(a)) {
        return Err(OpenTabFailure::BadArgs("인자에 제어 문자가 있습니다(줄바꿈·탭만 됨)".to_string()));
    }
    if let Some(title) = title {
        if !tab_title_ok(title) {
            return Err(OpenTabFailure::BadArgs("--title 은 한 줄, 100자까지입니다".to_string()));
        }
    }
    let size = argv.iter().map(|a| a.len() + 1).sum::<usize>() + cwd.len() + title.map_or(0, |t| t.len());
    if size > OPEN_TAB_MAX_BYTES {
        return Err(OpenTabFailure::BadArgs(format!(
            "요청이 너무 깁니다({} 바이트, 상한 {})",
            size, OPEN_TAB_MAX_BYTES
        )));
    }
    let info = claude_arg_info(argv);
    if let Some(name) = &info.name {
        if name.is_empty() || !paste_safe(name) || name.contains('\n') || name.contains('\t') {
            return Err(OpenTabFailure::BadArgs("--name 은 한 줄이어야 합니다".to_string()));
        }
        if let Some(title) = title {
            if name != title {
                return Err(OpenTabFailure::BadArgs(format!(
                    "--title({title})과 --name({name})이 다릅니다. 보드는 탭 이름을 세션 이름으로 씁니다. 하나만 주거나 같게 주세요"
                )));
            }
        }
    }
    let mut out = argv.to_vec();
    let mut session_id: Option<String> = None;
    let mut resumes = false;
    if let Some(resume) = &info.resume {
        if is_session_id(resume) && !info.fork {
            session_id = Some(resume.to_lowercase());
            resumes = true;
        }
    } else if let Some(flagged) = &info.session_flag {
        if is_session_id(flagged) {
            session_id = Some(flagged.to_lowercase());
        }
    } else if info.resume.is_none() && !info.picker && !info.continues && info.session_flag.is_none() {
        out.insert(1, "--session-id".to_string());
        out.insert(2, new_id.to_string());
        session_id = Some(new_id.to_string());
    }
    let name = info.name.clone().or_else(|| title.map(str::to_owned));
    Ok(OpenTabPlanInput { argv: out, session_id, resumes, name, cwd: cwd.to_string() })
}

/// 답을 꾸미는 md-render 플러그인 인자 자리 — 두 번째 인자부터 훑어 `--plugin-dir <md-render 폴더>`(두 낱말)와
/// `--plugin-dir=<md-render 폴더>`(한 낱말)의 자리를 모은다. Windows 는 폴더를 `\` 로 쓰므로 끝을 볼 때 `/` 로 바꿔 견준다.
/// mac `ShellCommand.swift` 의 mdRenderArgIndices 를 옮긴 것이다.
fn md_render_arg_indices(args: &[String]) -> HashSet<usize> {
    let normalized = |s: &str| s.replace('\\', "/");
    let mut out = HashSet::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--plugin-dir" && i + 1 < args.len() && normalized(&args[i + 1]).ends_with("/plugins/md-render") {
            out.insert(i);
            out.insert(i + 1);
            i += 2;
            continue;
        }
        if args[i].starts_with("--plugin-dir=") && normalized(&args[i]).ends_with("/plugins/md-render") {
            out.insert(i);
        }
        i += 1;
    }
    out
}

/// 답을 꾸미기(`--plugin-dir <md_render_dir>`)에 쓸 명령 꾸러기 — md-render 폴더 인자는 mac 답 뷰(mdrender.rs)가
/// 자기 폴더를 맨 뒤에 다시 넣어 뒤섞는 걸 막고자 지우고, 주어지면 맨 뒤에 하나만 넣는다. 사용자가 준 다른 `--plugin-dir` 는 남는다.
/// Windows 판은 셸 문자열을 만들지 않는다 — 낱말 그대로 내온다.
pub fn open_tab_command(argv: Vec<String>, md_render_dir: Option<&str>) -> Vec<String> {
    let md_render = md_render_arg_indices(&argv);
    let mut out: Vec<String> = argv
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !md_render.contains(i))
        .map(|(_, a)| a)
        .collect();
    if let Some(dir) = md_render_dir {
        out.push("--plugin-dir".to_string());
        out.push(dir.to_string());
    }
    out
}

/// 보드 탭 화면 하나의 지금 값 — 앱이 화면(셸)에서 떠 넘긴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTabScreen {
    /// 탭 id
    pub id: String,
    /// 화면이 돌리는 claude 의 세션 id, 새 세션이면 아직 없어 None
    pub session_id: Option<String>,
    /// 화면 셸의 지금 경로(앱이 바로 가기를 풀어 넘긴다)
    pub cwd: String,
    /// 짝지은 세션의 claude pid
    pub pid: Option<i32>,
    /// 이 화면 claude 의 이름들(지금 이름·바꿀 이름·방금 붙인 이름·명령의 --name·한 화면 탭의 탭 이름)
    pub names: Vec<String>,
    /// claude(또는 셸 위의 다른 프로그램)가 돌거나 막 넣는 중. 셸만 남은 화면·다시 뜬 뒤 아직 들어가지 않은 화면은 false
    pub busy: bool,
    /// 자율주행을 이어받은 화면 — 다른 명령을 넣지 않는다
    pub takeover: bool,
}

/// 보드 밖에서 도는 claude(살아 있는 것만) — 같은 세션·이름이 보드 탭 밖에서 돌면 두 번째 claude 를 띄우지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveClaude {
    pub pid: i32,
    pub session: Option<String>,
    pub name: Option<String>,
}

/// 이미 있는 탭 판정
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTabPlan {
    /// 새 탭
    Open,
    /// 이미 도는 화면 — 가리키기만 한다(앞으로만)
    Exists(String),
    /// 같은 폴더인데 claude 가 없다(셸만 남음·다시 뜨고 아직 들어가지 않음) — 그 화면에 띄운다
    Reuse(String),
    /// 띄우기를 거절함 — 세션(이름)이 보드 탭 밖 claude 나 다른 폴더 셸에 겹침
    Refuse(String),
}

/// 같은 세션 id(먼저)나 같은 이름(대소문자 무시)의 보드 탭 화면이 있으면 그 화면으로 — claude 가 돌거나 막 넣는 화면,
/// 방금 자율주행을 이어받은 화면·다른 폴더 셸은 가리키기만 한다. 같은 폴더의 셸만 남은 화면은 거기에 띄운다(끝난 세션 이어 열기).
/// 탭이 없는데 같은 세션·이름의 claude 가 살아 있으면 그 claude 가 든 보드 탭 화면을 가리키고, 보드 탭 밖이면 거절한다.
/// mac `planOpenTab`(OpenTab.swift 250-279)을 그대로 옮겼다.
pub fn plan_open_tab(input: &OpenTabPlanInput, screens: &[OpenTabScreen], live: &[LiveClaude]) -> OpenTabPlan {
    let key: Option<String> = input.name.as_ref().map(|n| n.to_lowercase());
    let matches_session =
        |s: &OpenTabScreen| match (&input.session_id, &s.session_id) {
            (Some(want), Some(sid)) => want == sid,
            _ => false,
        };
    let matches_name = |s: &OpenTabScreen,
                        key: &Option<String>| match key {
        Some(k) => s.names.iter().any(|n| n.to_lowercase() == *k),
        None => false,
    };
    let matched = screens
        .iter()
        .find(|s| matches_session(s))
        .or_else(|| screens.iter().find(|s| matches_name(s, &key)));
    if let Some(s) = matched {
        if s.busy || s.takeover {
            return OpenTabPlan::Exists(s.id.clone());
        }
        if s.cwd == input.cwd {
            return OpenTabPlan::Reuse(s.id.clone());
        }
    }
    for c in live {
        let by_session = match (&input.session_id, &c.session) {
            (Some(want), Some(sid)) => want == sid,
            _ => false,
        };
        let by_name = match (&key, &c.name) {
            (Some(k), Some(n)) => n.to_lowercase() == *k,
            _ => false,
        };
        if !by_session && !by_name {
            continue;
        }
        if let Some(s) = screens.iter().find(|s| s.pid == Some(c.pid)) {
            return OpenTabPlan::Exists(s.id.clone());
        }
        return if by_session {
            OpenTabPlan::Refuse(format!("이 세션이 보드 탭 밖(pid {})에서 돌고 있습니다", c.pid))
        } else {
            OpenTabPlan::Refuse(format!(
                "같은 이름({})의 세션이 보드 탭 밖(pid {})에서 돌고 있습니다",
                c.name.as_deref().unwrap_or(""),
                c.pid
            ))
        };
    }
    OpenTabPlan::Open
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// mac OpenTabTests 의 sid — 8-4-4-4-12 꼴 16진수
    const SID: &str = "0f0e1d2c-3b4a-5968-7766-554433221100";
    /// mac OpenTabTests 의 newID
    const NEW_ID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    /// mac 시험의 화면 id A
    const A: &str = "00000000-0000-0000-0000-00000000000a";
    /// mac 시험의 화면 id B
    const B: &str = "00000000-0000-0000-0000-00000000000b";
    /// mac 의 "/w" 에 해당하는 Windows 절대 경로
    const DEF_CWD: &str = "C:\\w";

    /// OpenTabFailure 의 why — 네 갈래 모두 같은 글자 칸이므로 견주는데 쓴다
    fn why_of(e: &OpenTabFailure) -> &str {
        match e {
            OpenTabFailure::BadArgs(why)
            | OpenTabFailure::BoardOff(why)
            | OpenTabFailure::Refused(why)
            | OpenTabFailure::Timeout(why) => why,
        }
    }

    // ── 요청 검사와 띄울 꼴(mac OpenTabTests.openTabPrepare 를 옮긴다) ──

    struct PrepRow {
        note: &'static str,
        title: Option<&'static str>,
        argv: Vec<String>,
        argv_out: Option<Vec<String>>,
        session: Option<&'static str>,
        name: Option<&'static str>,
        why: Option<&'static str>,
    }

    #[test]
    fn open_tab_prepare_checks_the_request_and_shapes_the_argv() {
        let long = "가".repeat(100_000);
        let mut rows = vec![
            PrepRow {
                note: "새 세션은 claude 바로 뒤에 --session-id",
                title: None,
                argv: s(&["claude", "--model", "opus", "첫 요청"]),
                argv_out: Some(s(&["claude", "--session-id", NEW_ID, "--model", "opus", "첫 요청"])),
                session: Some(NEW_ID),
                name: None,
                why: None,
            },
            PrepRow {
                note: "이어 열기는 그 세션(대문자 id 는 소문자로)",
                title: None,
                argv: s(&["claude", "--name", "역할-2", "--resume", "0F0E1D2C-3B4A-5968-7766-554433221100"]),
                argv_out: Some(s(&["claude", "--name", "역할-2", "--resume", "0F0E1D2C-3B4A-5968-7766-554433221100"])),
                session: Some(SID),
                name: Some("역할-2"),
                why: None,
            },
            PrepRow {
                note: "--resume=",
                title: Some("역할"),
                argv: s(&["claude", "--resume=0f0e1d2c-3b4a-5968-7766-554433221100"]),
                argv_out: Some(s(&["claude", "--resume=0f0e1d2c-3b4a-5968-7766-554433221100"])),
                session: Some(SID),
                name: Some("역할"),
                why: None,
            },
            PrepRow {
                note: "-r 값",
                title: None,
                argv: s(&["claude", "-r", SID, "-n", "역할"]),
                argv_out: Some(s(&["claude", "-r", SID, "-n", "역할"])),
                session: Some(SID),
                name: Some("역할"),
                why: None,
            },
            PrepRow {
                note: "고르기 --resume 은 세션을 모른다",
                title: None,
                argv: s(&["claude", "--resume", "--model", "opus"]),
                argv_out: Some(s(&["claude", "--resume", "--model", "opus"])),
                session: None,
                name: None,
                why: None,
            },
            PrepRow {
                note: "--continue 는 세션을 모른다",
                title: None,
                argv: s(&["claude", "-c"]),
                argv_out: Some(s(&["claude", "-c"])),
                session: None,
                name: None,
                why: None,
            },
            PrepRow {
                note: "갈래(--fork-session)는 새 세션이라 이어 연 id 가 아니다",
                title: None,
                argv: s(&["claude", "--resume", SID, "--fork-session"]),
                argv_out: Some(s(&["claude", "--resume", SID, "--fork-session"])),
                session: None,
                name: None,
                why: None,
            },
            PrepRow {
                note: "--session-id 를 주면 그 id",
                title: None,
                argv: s(&["claude", "--session-id", SID]),
                argv_out: Some(s(&["claude", "--session-id", SID])),
                session: Some(SID),
                name: None,
                why: None,
            },
            PrepRow {
                note: "이름 꼴 --resume 은 id 를 모른다",
                title: None,
                argv: s(&["claude", "--resume", "역할"]),
                argv_out: Some(s(&["claude", "--resume", "역할"])),
                session: None,
                name: None,
                why: None,
            },
            PrepRow {
                note: "첫 요청의 줄바꿈·탭은 된다",
                title: None,
                argv: s(&["claude", "한 줄\n두 줄\t탭"]),
                argv_out: Some(s(&["claude", "--session-id", NEW_ID, "한 줄\n두 줄\t탭"])),
                session: Some(NEW_ID),
                name: None,
                why: None,
            },
            PrepRow {
                note: "claude 가 아니면 거절",
                title: None,
                argv: s(&["bash", "-c", "rm -rf ~"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("claude 명령만"),
            },
            PrepRow {
                note: "경로 꼴 claude 도 거절",
                title: None,
                argv: s(&["C:\\x\\claude"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("claude 명령만"),
            },
            PrepRow {
                note: "빈 명령",
                title: None,
                argv: vec![],
                argv_out: None,
                session: None,
                name: None,
                why: Some("(없음)"),
            },
            PrepRow {
                note: "ESC(붙여넣기 끝 꼴)는 거절",
                title: None,
                argv: s(&["claude", "a\u{1B}[201~; rm -rf ~"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("제어 문자"),
            },
            PrepRow {
                note: "CR 은 거절",
                title: None,
                argv: s(&["claude", "a\rb"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("제어 문자"),
            },
            PrepRow {
                note: "C1 제어 문자는 거절",
                title: None,
                argv: s(&["claude", "a\u{9B}b"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("제어 문자"),
            },
            PrepRow {
                note: "탭 이름과 세션 이름이 다르면 거절",
                title: Some("가"),
                argv: s(&["claude", "--name", "나"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("다릅니다"),
            },
            PrepRow {
                note: "같으면 됨",
                title: Some("가"),
                argv: s(&["claude", "--name", "가"]),
                argv_out: Some(s(&["claude", "--session-id", NEW_ID, "--name", "가"])),
                session: Some(NEW_ID),
                name: Some("가"),
                why: None,
            },
            PrepRow {
                note: "여러 줄 탭 이름은 거절",
                title: Some("가\n나"),
                argv: s(&["claude"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("한 줄"),
            },
            PrepRow {
                note: "여러 줄 세션 이름은 거절",
                title: None,
                argv: s(&["claude", "--name", "가\n나"]),
                argv_out: None,
                session: None,
                name: None,
                why: Some("한 줄"),
            }
        ];
        // 너무 긴 요청은 표 밖에서(표 리터럴이 거대해지지 않게)
        rows.push(PrepRow {
            note: "너무 긴 요청",
            title: None,
            argv: vec!["claude".to_string(), long],
            argv_out: None,
            session: None,
            name: None,
            why: Some("너무 깁니다"),
        });
        for r in &rows {
            match open_tab_prepare(DEF_CWD, r.title, &r.argv, NEW_ID) {
                Ok(p) => {
                    let out = r.argv_out.as_ref().unwrap_or_else(|| panic!("받음({}): {:?}", r.note, p.argv));
                    assert_eq!(p.argv, *out, "{}", r.note);
                    assert_eq!(p.session_id.as_deref(), r.session, "{}", r.note);
                    assert_eq!(p.name.as_deref(), r.name, "{}", r.note);
                    assert_eq!(
                        p.resumes,
                        r.session.is_some()
                            && r.argv.iter().any(|a| a == "--resume" || a == "-r" || a.starts_with("--resume=")),
                        "{}",
                        r.note
                    );
                }
                Err(e) => {
                    assert!(r.argv_out.is_none(), "거절해야 하는데 받음({}) — {:?}: {}", r.note, e, why_of(&e));
                    assert!(why_of(&e).contains(r.why.unwrap_or("\u{0}")), "{}: {}", r.note, why_of(&e));
                }
            }
        }
    }

    #[test]
    fn open_tab_prepare_rejects_relative_and_controlled_cwd() {
        let e = open_tab_prepare("w", None, &s(&["claude"]), NEW_ID).unwrap_err();
        assert_eq!(why_of(&e), "--cwd 는 절대 경로여야 합니다: w");
        // 줄바꿈이 든 경로는 절대 경로여도 거절한다(mac 시험의 "/w\n/x")
        assert!(open_tab_prepare("C:\\w\nx", None, &s(&["claude"]), NEW_ID).is_err());
    }

    // ── claude 인자 읽기(mac OpenTabTests.argInfo 를 옮긴다) ──

    #[test]
    fn claude_arg_info_reads_name_resume_and_continue() {
        assert_eq!(
            claude_arg_info(&s(&["claude", "--name=가", "--resume", SID, "-c"])),
            ClaudeArgInfo {
                name: Some("가".to_string()),
                resume: Some(SID.to_string()),
                picker: false,
                continues: true,
                session_flag: None,
                fork: false,
            }
        );
        assert_eq!(claude_arg_info(&s(&["claude", "-r"])), ClaudeArgInfo { picker: true, ..ClaudeArgInfo::default() });
        // 첫 요청(위치 인자)에 든 옵션 꼴 글자는 한 인자라 옵션이 아니다
        assert_eq!(claude_arg_info(&s(&["claude", "--name 가"])), ClaudeArgInfo::default());
        // 첫 인자(claude)는 보지 않는다
        assert_eq!(claude_arg_info(&s(&["--name", "가"])), ClaudeArgInfo::default());
    }

    #[test]
    fn claude_arg_info_swallows_only_values_it_owns() {
        // 값 없는 --resume·-r 을 고르기로 본다
        assert!(claude_arg_info(&s(&["claude", "--resume"])).picker);
        assert!(claude_arg_info(&s(&["claude", "-r"])).picker);
        // 다음 인자가 - 로 시작하면 값을 먹지 않는다 — 그 옵션은 독자 인자로 남는다
        assert!(claude_arg_info(&s(&["claude", "--name", "--model", "opus"])).name.is_none());
        assert!(claude_arg_info(&s(&["claude", "--session-id"])).session_flag.is_none());
        // inline 꼴 --session-id=<id>·--resume=<id> 도 값으로 읽는다
        assert_eq!(
            claude_arg_info(&s(&["claude", "--session-id=0f0e1d2c-3b4a-5968-7766-554433221100"])).session_flag,
            Some(SID.to_string())
        );
        assert_eq!(claude_arg_info(&s(&["claude", "-n", "역할"])).name, Some("역할".to_string()));
        // -n=가 는 -- 로 시작하지 않은 짧은 꼴이라 낱말째 하나다
        assert!(claude_arg_info(&s(&["claude", "-n=가"])).name.is_none());
        // --fork-session
        assert!(claude_arg_info(&s(&["claude", "--fork-session", "--resume", SID])).fork);
        assert_eq!(claude_arg_info(&s(&["claude"])), ClaudeArgInfo::default());
    }

    #[test]
    fn is_session_id_checks_the_uuid_shape() {
        assert!(is_session_id(SID));
        assert!(is_session_id("0F0E1D2C-3B4A-5968-7766-554433221100"));
        assert!(is_session_id(NEW_ID));
        assert!(!is_session_id("역할"));
        assert!(!is_session_id(""));
        assert!(!is_session_id("0f0e-3b4a-5968-7766-554433221100"));
        // 자리수가 어긋나면 아니다
        assert!(!is_session_id("0f0e1d2c-3b4a-5968-7766-5544332211000"));
        // g 가 아닌 글자
        assert!(!is_session_id("0f0e1d2c-3b4a-5968-7766-g54433221100"));
    }

    #[test]
    fn tab_title_ok_takes_one_line_up_to_100_letters() {
        assert!(tab_title_ok("가"));
        assert!(tab_title_ok("역할 2"));
        assert!(!tab_title_ok("줄\n바꿈"));
        assert!(!tab_title_ok("탭\t끼움"));
        assert!(!tab_title_ok("a\u{1B}[201~"));
        assert!(!tab_title_ok("a\u{2028}b"));
        assert!(!tab_title_ok(""));
        // 경계 — 100자까지, 101자부터 거절
        assert!(tab_title_ok("가".repeat(100).as_str()));
        assert!(!tab_title_ok("가".repeat(101).as_str()));
    }

    // ── 이미 있는 탭 가리기(mac OpenTabTests.plan 를 옮긴다) ──

    fn screen(id: &str, session: Option<&str>) -> OpenTabScreen {
        OpenTabScreen {
            id: id.to_string(),
            session_id: session.map(str::to_string),
            cwd: DEF_CWD.to_string(),
            pid: None,
            names: vec![],
            busy: false,
            takeover: false,
        }
    }

    fn live(pid: i32, session: Option<&str>, name: Option<&str>) -> LiveClaude {
        LiveClaude { pid, session: session.map(str::to_string), name: name.map(str::to_string) }
    }

    fn input(session: Option<&str>, name: Option<&str>) -> OpenTabPlanInput {
        OpenTabPlanInput {
            argv: vec![],
            session_id: session.map(str::to_string),
            resumes: false,
            name: name.map(str::to_string),
            cwd: DEF_CWD.to_string(),
        }
    }

    struct PlanRow {
        note: &'static str,
        session: Option<&'static str>,
        name: Option<&'static str>,
        screens: Vec<OpenTabScreen>,
        live: Vec<LiveClaude>,
        want: OpenTabPlan,
    }

    #[test]
    fn plan_open_tab_points_at_the_existing_tab_or_refuses() {
        let other_cwd = "C:\\other".to_string();
        let mut running_b = screen(B, Some(SID));
        running_b.pid = Some(10);
        running_b.busy = true;
        let mut named_a = screen(A, Some(SID));
        named_a.pid = Some(10);
        named_a.names = s(&["role-2"]);
        named_a.busy = true;
        let mut session_screen_other_cwd = screen(A, Some(SID));
        session_screen_other_cwd.cwd = other_cwd.clone();
        session_screen_other_cwd.pid = Some(10);
        session_screen_other_cwd.busy = true;
        let mut taken_over_a = screen(A, Some(SID));
        taken_over_a.takeover = true;
        let mut outside_a = screen(A, None);
        outside_a.pid = Some(11);
        outside_a.busy = true;
        let mut holder_a = screen(A, None);
        holder_a.pid = Some(20);
        holder_a.busy = true;
        let mut holder_b = screen(B, None);
        holder_b.pid = Some(21);
        holder_b.busy = true;
        let mut busy_a = screen(A, None);
        busy_a.busy = true;
        let rows = [
            PlanRow {
                note: "아무것도 없으면 새 탭",
                session: Some(SID),
                name: Some("역할"),
                screens: vec![],
                live: vec![],
                want: OpenTabPlan::Open,
            },
            PlanRow {
                note: "같은 세션이 도는 화면은 앞으로만",
                session: Some(SID),
                name: None,
                screens: vec![screen(A, None), running_b],
                live: vec![live(10, Some(SID), None)],
                want: OpenTabPlan::Exists(B.to_string()),
            },
            PlanRow {
                note: "같은 세션인데 claude 가 끝나 셸만 남은 화면은 그 화면에",
                session: Some(SID),
                name: None,
                screens: vec![screen(B, Some(SID))],
                live: vec![],
                want: OpenTabPlan::Reuse(B.to_string()),
            },
            PlanRow {
                note: "같은 이름(대소문자 무시)이 도는 화면은 앞으로만",
                session: Some(NEW_ID),
                name: Some("Role-2"),
                screens: vec![named_a],
                live: vec![live(10, None, Some("role-2"))],
                want: OpenTabPlan::Exists(A.to_string()),
            },
            PlanRow {
                note: "세션 id 가 이름보다 먼저",
                session: Some(SID),
                name: Some("역할"),
                screens: vec![{
                    let mut a = screen(A, None);
                    a.names = s(&["역할"]);
                    a.busy = true;
                    a
                }, screen(B, Some(SID))],
                live: vec![],
                want: OpenTabPlan::Reuse(B.to_string()),
            },
            PlanRow {
                note: "셸만 남은 같은 이름의 화면이 다른 폴더면 새 탭",
                session: Some(NEW_ID),
                name: Some("역할"),
                screens: vec![{
                    let mut a = screen(A, None);
                    a.cwd = other_cwd.clone();
                    a.names = s(&["역할"]);
                    a
                }],
                live: vec![],
                want: OpenTabPlan::Open,
            },
            PlanRow {
                note: "다른 폴더여도 claude 가 돌면 앞으로만",
                session: Some(SID),
                name: None,
                screens: vec![session_screen_other_cwd],
                live: vec![],
                want: OpenTabPlan::Exists(A.to_string()),
            },
            PlanRow {
                note: "이어받기 화면은 셸만 남아도 다른 명령을 넣지 않는다",
                session: Some(SID),
                name: None,
                screens: vec![taken_over_a],
                live: vec![],
                want: OpenTabPlan::Exists(A.to_string()),
            },
            PlanRow {
                note: "보드 탭 밖에서 같은 세션이 돌면 거절",
                session: Some(SID),
                name: None,
                screens: vec![outside_a],
                live: vec![live(20, Some(SID), None)],
                want: OpenTabPlan::Refuse("이 세션이 보드 탭 밖(pid 20)에서 돌고 있습니다".to_string()),
            },
            PlanRow {
                note: "보드 탭 밖에서 같은 이름이 돌면 거절",
                session: Some(NEW_ID),
                name: Some("역할"),
                screens: vec![],
                live: vec![live(21, None, Some("역할"))],
                want: OpenTabPlan::Refuse("같은 이름(역할)의 세션이 보드 탭 밖(pid 21)에서 돌고 있습니다".to_string()),
            },
            PlanRow {
                note: "이름이 아직 화면 값에 없어도 그 claude 가 든 보드 탭 화면이면 앞으로만",
                session: Some(NEW_ID),
                name: Some("역할"),
                screens: vec![holder_a, holder_b],
                live: vec![live(21, None, Some("역할"))],
                want: OpenTabPlan::Exists(B.to_string()),
            },
            PlanRow {
                note: "세션도 마찬가지",
                session: Some(SID),
                name: None,
                screens: vec![{
                    let mut a = screen(A, None);
                    a.pid = Some(21);
                    a.busy = true;
                    a
                }],
                live: vec![live(21, Some(SID), None)],
                want: OpenTabPlan::Exists(A.to_string()),
            },
            PlanRow {
                note: "이름도 세션도 없으면 새 탭",
                session: None,
                name: None,
                screens: vec![busy_a],
                live: vec![live(1, None, Some("x"))],
                want: OpenTabPlan::Open,
            },
            PlanRow {
                note: "세션 id 없는 화면은 세션으로 맞지 않는다",
                session: None,
                name: None,
                screens: vec![screen(A, None)],
                live: vec![],
                want: OpenTabPlan::Open,
            },
        ];
        for r in &rows {
            let got = plan_open_tab(&input(r.session, r.name), &r.screens, &r.live);
            assert_eq!(got, r.want, "{}", r.note);
        }
    }

    // ── 답을 꾸미는 명령 꾸러기(mac ShellCommandTests.openTabMdRender·openTabAttack 을 옮긴다) ──
    // mac 은 명령마다 따옴표로 묶어 셸에 치고 다시 읽어 같은 인자인지 확인하지만, Windows 판은 셸 문자열을 만들지 않는다 —
    // 명령이 낱말 argv 로 넘어가고, 막아야 할 위험(셸 낱말에 섞인 md-render·공격 낱말)은 그대로 지나가는 것으로 답한다

    /// mac ShellCommandTests.openTabMdRender — 요청의 md-render --plugin-dir 은 빼고 보드 경로로
    #[test]
    fn open_tab_command_drops_request_md_render_and_appends_its_own() {
        let argv = s(&["claude", "--plugin-dir", "/old/plugins/md-render", "--plugin-dir=/b/plugins/md-render", "--plugin-dir", "/mine", "글"]);
        let dir = "C:\\h\\.claude\\notifier\\plugins\\md-render";
        assert_eq!(
            open_tab_command(argv.clone(), Some(dir)),
            s(&["claude", "--plugin-dir", "/mine", "글", "--plugin-dir", dir]),
        );
        assert_eq!(open_tab_command(argv, None), s(&["claude", "--plugin-dir", "/mine", "글"]));
        // = 으로 시작하는 낱말도 낱말 그대로 지나간다
        assert_eq!(open_tab_command(s(&["claude", "=x"]), None), s(&["claude", "=x"]));
    }

    struct AttackRow {
        note: &'static str,
        argv: Vec<String>,
    }

    fn attack_rows() -> Vec<AttackRow> {
        let sid = SID;
        vec![
            AttackRow {
                note: "첫 요청 글 안의 --plugin-dir '…/plugins/md-render'(따옴표 꼴)",
                argv: s(&["claude", "--session-id", sid, "x --plugin-dir '/x/plugins/md-render' ; touch /tmp/cb-pwned #"]),
            },
            AttackRow {
                note: "첫 요청 글 안의 --plugin-dir …/plugins/md-render(따옴표 없는 꼴)",
                argv: s(&["claude", "--session-id", sid, "보세요 --plugin-dir /x/plugins/md-render 끝"]),
            },
            AttackRow {
                note: "'\"'\"' 꼴을 글로",
                argv: s(&["claude", "--session-id", sid, "a '\"'\"' --plugin-dir='/x/plugins/md-render' b"]),
            },
            AttackRow {
                note: "글 끝의 --session-id 값이 닫는 따옴표를 삼키게(3.3.2 되살리기에서 다음 인자가 명령이 됨)",
                argv: s(&["claude", "--session-id", sid, "요약해 줘 --session-id x", "; touch /tmp/cb-pwned #"]),
            },
            AttackRow {
                note: "--session-id= 꼴",
                argv: s(&["claude", "--session-id", sid, "a --session-id=x", "; touch /tmp/cb-pwned #"]),
            },
            AttackRow {
                note: "--name 글",
                argv: s(&["claude", "--session-id", sid, "use -n flag --name 'x' ; touch /tmp/cb-pwned"]),
            },
            AttackRow {
                note: "; 와 $(…)",
                argv: s(&["claude", "--session-id", sid, "$(touch /tmp/cb-pwned); `id` && echo"]),
            },
            AttackRow {
                note: "' 뒤 결합 문자(3.3.2 shlexQuote 가 ' 를 못 바꿈)",
                argv: s(&["claude", "--session-id", sid, "요약 '\u{301}; touch /tmp/cb-pwned; : '\u{301}"]),
            },
            AttackRow {
                note: "--resume 글은 이어 열기가 아니다",
                argv: s(&["claude", "--session-id", sid, "--resume 하지 마 -c"]),
            },
            AttackRow {
                note: "줄바꿈·탭이 든 글",
                argv: s(&["claude", "--session-id", sid, "첫 줄\n; touch /tmp/cb-pwned\t#"]),
            },
        ]
    }

    #[test]
    fn open_tab_command_passes_attack_words_untouched_and_adds_md_render_last() {
        let dir = "C:\\h\\.claude\\notifier\\plugins\\md-render";
        for r in attack_rows() {
            let mut want = r.argv.clone();
            want.push("--plugin-dir".to_string());
            want.push(dir.to_string());
            assert_eq!(open_tab_command(r.argv.clone(), Some(dir)), want, "{}", r.note);
            // 막지 않는다 — 낱말 안 낱말은 그대로 지나간다
            assert_eq!(open_tab_command(r.argv.clone(), None), r.argv, "{}", r.note);
        }
    }
}
