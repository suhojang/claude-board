//! 시험용 claude 바꿔치기 — 환경변수 `CLAUDE_BOARD_CLAUDE`(`|` 로 이은 절대 경로: 프로그램과 앞쪽 인자)가 있고
//! `CLAUDE_BOARD_HOME` 도 있을 때만 보드가 치는 명령의 맨 앞 `claude` 를 그 값으로 바꾼다. 진짜 claude 를 띄우지 않고 명령 길을 시험하려는 장치다.
//!
//! 규칙: 두 변수가 모두 있고, `|` 로 가른 조각이 모두 비지 않은 절대 경로이며 실재할 때만 켜진다.
//! 하나라도 어긋나면 꺼진 채(None)로 두고 board.log 에 한 줄 남긴다 — 값이 없으면(`CLAUDE_BOARD_CLAUDE` 미설정) 아무것도 남기지 않는다.
//! 홈을 따로 정한 실행에서만 켜지게 해서, 사용자 보드에 남은 환경변수 하나가 실수로 진짜 claude 를 바꿔치지 못하게 한다.
//! 바꾸는 쪽은 프론트(`terminal/testClaude.ts`)다. 보드가 직접 띄우는 곳(계정 추가 로그인)은 `login_program` 으로 같은 값을 쓴다.
//!
//! 닫힌 채로 실패한다 — 꺼짐(Off) / 켜짐(On) / 어긋남(Rejected) 셋으로 가른다. 어긋남이면 진짜 claude 를 찾지도 띄우지도 않는다.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::util::board_log;

pub const CLAUDE_ENV: &str = "CLAUDE_BOARD_CLAUDE";
pub const HOME_ENV: &str = claude_board_core::paths::HOME_ENV;

/// `prefix` 의 판정 — 꺼짐 / 켜짐 / 켜려 했으나 어긋남(이유)
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Off,
    On(Vec<PathBuf>),
    Rejected(String),
}

fn non_empty(env: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<OsString> {
    env(key).filter(|v| !v.is_empty())
}

/// 환경(`env`)과 파일 확인(`exists`)만으로 정하는 순수 판정 — 시험은 가짜 환경으로 이것을 부른다.
pub fn decide(env: &dyn Fn(&str) -> Option<OsString>, exists: &dyn Fn(&Path) -> bool) -> Decision {
    let Some(raw) = non_empty(env, CLAUDE_ENV) else { return Decision::Off };
    if non_empty(env, HOME_ENV).is_none() {
        return Decision::Rejected(format!("{CLAUDE_ENV} 는 {HOME_ENV} 도 있을 때만 쓴다 — 무시한다"));
    }
    let Some(raw) = raw.to_str() else { return Decision::Rejected(format!("{CLAUDE_ENV} 가 유니코드가 아니다 — 무시한다")) };
    let mut parts = Vec::new();
    for piece in raw.split('|') {
        if piece.is_empty() {
            return Decision::Rejected(format!("{CLAUDE_ENV} 에 빈 조각이 있다 — 무시한다"));
        }
        if piece.contains('"') {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각에 큰따옴표가 있다({piece}) — 무시한다"));
        }
        let path = PathBuf::from(piece);
        if !path.is_absolute() {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각이 절대 경로가 아니다({piece}) — 무시한다"));
        }
        if !exists(&path) {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각이 없는 파일이다({piece}) — 무시한다"));
        }
        parts.push(path);
    }
    Decision::On(parts)
}

/// 주어진 환경에서의 판정 — 어긋나면 board.log 한 줄을 남긴다.
pub fn decision(env: &dyn Fn(&str) -> Option<OsString>) -> Decision {
    let decided = decide(env, &|p| p.exists());
    if let Decision::Rejected(why) = &decided {
        board_log(&format!("[시험 claude] {why}"));
    }
    decided
}

/// 주어진 환경에서의 바꿔칠 값. 꺼짐과 어긋남 모두 None 이다 — 둘을 가르려면 `decision` 을 쓴다.
#[cfg(test)]
pub fn prefix(env: &dyn Fn(&str) -> Option<OsString>) -> Option<Vec<PathBuf>> {
    match decision(env) {
        Decision::On(parts) => Some(parts),
        Decision::Off | Decision::Rejected(_) => None,
    }
}

/// 이 프로세스의 환경에서의 판정.
pub fn decision_from_process() -> Decision {
    decision(&|key| std::env::var_os(key))
}

/// 보드가 claude 를 직접 띄우는 곳(계정 추가 로그인)이 쓸 프로그램과 앞쪽 인자.
/// 꺼짐이면 `find` 로 진짜 claude 를 찾고, 켜짐이면 가짜(프로그램 + 앞쪽 인자), 어긋남이면 진짜를 찾지 않고 거절한다.
pub fn login_program(decided: Decision, find: impl FnOnce() -> Option<PathBuf>) -> Result<(PathBuf, Vec<String>), String> {
    match decided {
        Decision::Off => find().map(|exe| (exe, Vec::new())).ok_or_else(|| "claude 명령을 찾지 못했습니다".to_string()),
        Decision::On(parts) => {
            let mut parts = parts.into_iter();
            let program = parts.next().ok_or_else(|| format!("{CLAUDE_ENV} 가 비었습니다"))?;
            Ok((program, parts.map(|p| p.to_string_lossy().into_owned()).collect()))
        }
        Decision::Rejected(why) => Err(format!("시험 claude 설정이 어긋나 계정 추가를 하지 않습니다 — {why}")),
    }
}

/// 프론트로 가는 답 — 꺼짐은 null, 켜짐은 조각 배열, 어긋남은 `{"invalid": 이유}`.
/// 어긋남을 꺼짐과 가르는 까닭: 프론트가 꺼짐으로 읽으면 진짜 claude 를 치게 된다.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum PrefixAnswer {
    Off,
    On(Vec<String>),
    Invalid { invalid: String },
}

impl From<Decision> for PrefixAnswer {
    fn from(decided: Decision) -> Self {
        match decided {
            Decision::Off => PrefixAnswer::Off,
            Decision::On(parts) => PrefixAnswer::On(parts.iter().map(|p| p.to_string_lossy().into_owned()).collect()),
            Decision::Rejected(why) => PrefixAnswer::Invalid { invalid: why },
        }
    }
}

/// 프론트의 `testClaudePrefix()` — 바꿔칠 프로그램·앞쪽 인자.
#[tauri::command(async)]
pub fn test_claude_prefix() -> PrefixAnswer {
    decision_from_process().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
        move |key| map.get(key).cloned()
    }

    fn rejected(d: &Decision) -> &str {
        match d {
            Decision::Rejected(why) => why,
            other => panic!("거절이어야 한다: {other:?}"),
        }
    }

    // 실재하는 절대 경로 둘 — 임시 폴더 안 파일
    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("cb-test-claude-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("claude.exe");
        let script = dir.join("fake-claude-tty.cjs");
        fs::write(&exe, b"").unwrap();
        fs::write(&script, b"").unwrap();
        (dir, exe, script)
    }

    fn always(_: &Path) -> bool {
        true
    }

    #[test]
    fn unset_is_off() {
        assert_eq!(decide(&env_of(&[]), &always), Decision::Off);
        assert_eq!(decide(&env_of(&[(HOME_ENV, "C:\\h")]), &always), Decision::Off);
        assert_eq!(decide(&env_of(&[(CLAUDE_ENV, ""), (HOME_ENV, "C:\\h")]), &always), Decision::Off);
    }

    #[test]
    fn inactive_unless_both_vars_are_set() {
        let (_dir, exe, _) = fixture();
        let claude = exe.to_str().unwrap();
        let only_claude = decide(&env_of(&[(CLAUDE_ENV, claude)]), &always);
        assert!(rejected(&only_claude).contains(HOME_ENV));
        let empty_home = decide(&env_of(&[(CLAUDE_ENV, claude), (HOME_ENV, "")]), &always);
        assert!(rejected(&empty_home).contains(HOME_ENV));
        assert_eq!(prefix(&env_of(&[(CLAUDE_ENV, claude)])), None);
    }

    #[test]
    fn relative_part_is_rejected() {
        let (_dir, exe, _) = fixture();
        let abs = exe.to_str().unwrap();
        for bad in ["claude.exe", "fake\\claude.exe", ".\\claude.exe", "..\\claude.exe", "\\claude.exe", "C:claude.exe"] {
            let d = decide(&env_of(&[(CLAUDE_ENV, bad), (HOME_ENV, "C:\\h")]), &always);
            assert!(rejected(&d).contains("절대 경로"), "{bad}: {d:?}");
            // 앞쪽 인자 자리의 상대 경로도 거절 — 하나라도 어긋나면 통째로 꺼진다
            let joined = format!("{abs}|{bad}");
            let d = decide(&env_of(&[(CLAUDE_ENV, joined.as_str()), (HOME_ENV, "C:\\h")]), &always);
            assert!(rejected(&d).contains("절대 경로"), "{joined}: {d:?}");
        }
    }

    #[test]
    fn missing_file_and_empty_piece_and_quote_are_rejected() {
        let (_dir, exe, _) = fixture();
        let abs = exe.to_str().unwrap();
        let ghost = exe.with_file_name("no-such-file.exe");
        let ghost = ghost.to_str().unwrap();
        let home = (HOME_ENV, "C:\\h");
        let missing = decide(&env_of(&[(CLAUDE_ENV, ghost), home]), &|p| p.exists());
        assert!(rejected(&missing).contains("없는 파일"));
        let second_missing = format!("{abs}|{ghost}");
        let missing = decide(&env_of(&[(CLAUDE_ENV, second_missing.as_str()), home]), &|p| p.exists());
        assert!(rejected(&missing).contains("없는 파일"));
        for bad in [format!("{abs}|"), format!("|{abs}"), format!("{abs}||{abs}")] {
            let d = decide(&env_of(&[(CLAUDE_ENV, bad.as_str()), home]), &always);
            assert!(rejected(&d).contains("빈 조각"), "{bad}: {d:?}");
        }
        let quoted = format!("\"{abs}\"");
        let d = decide(&env_of(&[(CLAUDE_ENV, quoted.as_str()), home]), &always);
        assert!(rejected(&d).contains("큰따옴표"));
    }

    #[test]
    fn valid_values_keep_order_and_spaces() {
        let (dir, exe, script) = fixture();
        let spaced = dir.join("홍 길동");
        fs::create_dir_all(&spaced).unwrap();
        let tool = spaced.join("claude.exe");
        fs::write(&tool, b"").unwrap();
        let joined = format!("{}|{}", exe.display(), script.display());
        let env = env_of(&[(CLAUDE_ENV, joined.as_str()), (HOME_ENV, "C:\\h")]);
        assert_eq!(decide(&env, &|p| p.exists()), Decision::On(vec![exe.clone(), script.clone()]));
        assert_eq!(prefix(&env), Some(vec![exe, script]));
        let one = tool.to_str().unwrap();
        assert_eq!(prefix(&env_of(&[(CLAUDE_ENV, one), (HOME_ENV, "C:\\h")])), Some(vec![tool]));
    }

    #[test]
    fn does_not_touch_the_process_environment_without_the_vars() {
        // 이 프로세스에는 두 변수가 없어야 한다 — 있으면 다른 시험이 샌 것이다
        assert!(std::env::var_os(CLAUDE_ENV).is_none());
        assert_eq!(test_claude_prefix(), PrefixAnswer::Off);
    }

    #[test]
    fn answer_tells_off_on_and_invalid_apart() {
        let (_dir, exe, _) = fixture();
        let claude = exe.to_str().unwrap();
        let json = |d: Decision| serde_json::to_string(&PrefixAnswer::from(d)).unwrap();
        assert_eq!(json(decision(&env_of(&[]))), "null");
        let on = decision(&env_of(&[(CLAUDE_ENV, claude), (HOME_ENV, "C:\\h")]));
        assert_eq!(json(on), serde_json::to_string(&[claude]).unwrap());
        // 홈 변수 없이 claude 변수만 — 꺼짐과 달라야 한다
        let bad = json(decision(&env_of(&[(CLAUDE_ENV, claude)])));
        assert!(bad.starts_with("{\"invalid\":"), "{bad}");
    }

    #[test]
    fn login_program_never_falls_back_to_real_claude_when_invalid() {
        let real = || Some(PathBuf::from("C:\\real\\claude.exe"));
        let never = || -> Option<PathBuf> { panic!("어긋남인데 진짜 claude 를 찾았다") };
        assert_eq!(login_program(Decision::Off, real), Ok((PathBuf::from("C:\\real\\claude.exe"), vec![])));
        assert!(login_program(Decision::Off, || None).is_err());
        let fake = Decision::On(vec![PathBuf::from("C:\\f\\node.exe"), PathBuf::from("C:\\f\\fake.cjs")]);
        assert_eq!(login_program(fake, never), Ok((PathBuf::from("C:\\f\\node.exe"), vec!["C:\\f\\fake.cjs".to_string()])));
        let err = login_program(Decision::Rejected("상대 경로".into()), never).unwrap_err();
        assert!(err.contains("상대 경로"), "{err}");
    }
}
