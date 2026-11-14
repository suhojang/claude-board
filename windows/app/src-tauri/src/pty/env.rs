//! 터미널 셸에 넘길 환경 블록.
//!
//! 보드 프로세스의 환경을 물려주지 않고 사용자 계정의 환경(레지스트리의 시스템·사용자 환경변수)에서 새로 만든다.
//! 보드가 낡은 PATH 로 떴거나 부모 Claude 세션의 `CLAUDECODE`·`CLAUDE_CODE_CHILD_SESSION` 을 물려받아도 셸에는 가지 않는다.

/// `이름=값\0…\0\0` 블록을 (이름, 값) 목록으로. `=C:` 처럼 이름이 `=` 로 시작하는 항목은 그 이름 그대로 둔다.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse(block: &[u16]) -> Vec<(String, String)> {
    block
        .split(|&c| c == 0)
        .take_while(|e| !e.is_empty())
        .filter_map(|e| {
            let s = String::from_utf16_lossy(e);
            // 이름 첫 글자가 `=` 일 수 있으니 두 번째 글자부터 `=` 를 찾는다
            let eq = s.char_indices().skip(1).find(|&(_, c)| c == '=')?.0;
            Some((s[..eq].to_string(), s[eq + 1..].to_string()))
        })
        .collect()
}

/// 블록에 변수를 더하거나 같은 이름(대소문자 무시)이 있으면 값을 바꾼다 — 계정 추가 로그인처럼 한 프로세스에만 다른 환경을 줄 때 쓴다.
/// 결과는 이름을 대소문자 무시로 정렬한 블록(끝의 이중 NUL 포함).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn with_vars(block: &[u16], vars: &[(String, String)]) -> Vec<u16> {
    let mut entries = parse(block);
    for (name, value) in vars {
        match entries.iter_mut().find(|(existing, _)| existing.eq_ignore_ascii_case(name)) {
            Some(entry) => *entry = (name.clone(), value.clone()),
            None => entries.push((name.clone(), value.clone())),
        }
    }
    entries.sort_by_key(|(name, _)| name.to_uppercase());
    let mut out: Vec<u16> = Vec::new();
    for (name, value) in &entries {
        out.extend(format!("{name}={value}").encode_utf16());
        out.push(0);
    }
    out.push(0);
    out
}

/// 보드 탭의 셸에 심는 표시 — 설치·제거 스크립트가 이 표시를 보면 보드를 끄지 않고 파일만 바꿔 끼운다(mac 의 보드 탭 설치 길과 같은 뜻).
pub const BOARD_TAB_VAR: (&str, &str) = ("CLAUDE_BOARD_TAB", "1");

/// 블록에 보드 탭 표시를 더한다(이미 같은 이름이 있으면 `1` 로 바꾼다).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn mark_board_tab(block: &[u16]) -> Vec<u16> {
    with_vars(block, &[(BOARD_TAB_VAR.0.to_string(), BOARD_TAB_VAR.1.to_string())])
}

/// 이 사용자 계정의 환경 블록(끝의 이중 NUL 포함). 보드가 띄우는 셸의 블록이므로 `CLAUDE_BOARD_TAB=1` 이 들어 있다.
/// 실패는 호출한 쪽이 터미널을 못 여는 오류로 올린다.
#[cfg(windows)]
pub fn new_block() -> std::io::Result<Vec<u16>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = null_mut();
    // SAFETY: 현재 프로세스 가짜 핸들로 토큰을 열고, 아래에서 한 번 닫는다
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut raw = null_mut();
    // SAFETY: bInherit=FALSE — 이 프로세스의 환경은 섞지 않는다. 돌려받은 블록은 복사한 뒤 바로 해제한다
    let made = unsafe { CreateEnvironmentBlock(&mut raw, token, FALSE) };
    let err = std::io::Error::last_os_error();
    // SAFETY: OpenProcessToken 이 연 토큰
    unsafe { CloseHandle(token) };
    if made == 0 || raw.is_null() {
        return Err(err);
    }
    let p = raw as *const u16;
    let mut len = 0;
    // SAFETY: 블록은 이중 NUL 로 끝난다. 끝 NUL 둘까지 센다
    unsafe {
        while !(*p.add(len) == 0 && *p.add(len + 1) == 0) {
            len += 1;
        }
    }
    // SAFETY: 위에서 센 길이(+끝 NUL 둘) 만큼 읽을 수 있다
    let block = unsafe { std::slice::from_raw_parts(p, len + 2) }.to_vec();
    // SAFETY: CreateEnvironmentBlock 이 준 블록
    unsafe { DestroyEnvironmentBlock(raw) };
    Ok(mark_board_tab(&block))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(entries: &[&str]) -> Vec<u16> {
        let mut b: Vec<u16> = entries.iter().flat_map(|e| e.encode_utf16().chain(std::iter::once(0))).collect();
        b.push(0);
        b
    }

    #[test]
    fn parse_splits_on_first_equals_after_name_start() {
        let got = parse(&block(&["=C:=C:\\", "Path=C:\\bin;D:\\x", "A=b=c", "빈=", "한글=값"]));
        let got: Vec<(&str, &str)> = got.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(got, [("=C:", "C:\\"), ("Path", "C:\\bin;D:\\x"), ("A", "b=c"), ("빈", ""), ("한글", "값")]);
    }

    #[test]
    fn with_vars_adds_replaces_case_insensitively_and_keeps_the_block_shape() {
        let base = block(&["=C:=C:\\", "Path=C:\\bin", "claude_config_dir=C:\\old", "Z=1"]);
        let got = with_vars(&base, &[("CLAUDE_CONFIG_DIR".into(), "C:\\새 폴더\\login-1".into()), ("BROWSER".into(), "C:\\x\\open.cmd".into())]);
        assert_eq!(got.iter().rev().take(2).collect::<Vec<_>>(), [&0, &0], "끝은 이중 NUL");
        let vars = parse(&got);
        let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["=C:", "BROWSER", "CLAUDE_CONFIG_DIR", "Path", "Z"]);
        assert_eq!(vars.iter().find(|(k, _)| k == "CLAUDE_CONFIG_DIR").unwrap().1, "C:\\새 폴더\\login-1");
        assert_eq!(vars.iter().find(|(k, _)| k == "Path").unwrap().1, "C:\\bin");
        // 더할 것이 없으면 그대로 읽힌다
        assert_eq!(parse(&with_vars(&base, &[])).len(), 4);
    }

    #[test]
    fn mark_board_tab_adds_the_variable_and_overrides_an_inherited_one() {
        let got = parse(&mark_board_tab(&block(&["Path=C:\\bin", "claude_board_tab=0"])));
        let tab: Vec<&(String, String)> = got.iter().filter(|(k, _)| k.eq_ignore_ascii_case("CLAUDE_BOARD_TAB")).collect();
        assert_eq!(tab.len(), 1, "같은 이름이 둘이면 안 된다: {got:?}");
        assert_eq!(tab[0].1, "1");
        assert!(got.iter().any(|(k, v)| k == "Path" && v == "C:\\bin"), "다른 변수는 그대로");
        let plain = parse(&mark_board_tab(&block(&["Path=C:\\bin"])));
        assert!(plain.contains(&("CLAUDE_BOARD_TAB".to_string(), "1".to_string())));
    }

    #[test]
    fn parse_stops_at_the_double_nul() {
        let mut b = block(&["A=1"]);
        b.extend("B=2".encode_utf16());
        assert_eq!(parse(&b).len(), 1);
    }

    /// 8. 새 블록에는 PATH 가 있고, 이 시험이 보드 환경에 심은 Claude 세션 변수는 없다.
    #[cfg(windows)]
    #[test]
    fn new_block_has_path_and_drops_inherited_claude_vars() {
        std::env::set_var("CLAUDECODE", "1");
        std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1");
        std::env::set_var("CB_TEST_ONLY_PROCESS_VAR", "x");
        let vars = parse(&new_block().expect("환경 블록"));
        let has = |name: &str| vars.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
        assert!(has("PATH"), "PATH 가 없다: {} 개 항목", vars.len());
        assert!(has("SystemRoot"), "SystemRoot 가 없다");
        assert!(vars.contains(&("CLAUDE_BOARD_TAB".to_string(), "1".to_string())), "보드 탭 표시가 없다");
        assert!(!has("CLAUDECODE"), "CLAUDECODE 가 새어 들어갔다");
        assert!(!has("CLAUDE_CODE_CHILD_SESSION"), "CLAUDE_CODE_CHILD_SESSION 이 새어 들어갔다");
        assert!(!has("CB_TEST_ONLY_PROCESS_VAR"), "이 프로세스에만 있던 변수가 새어 들어갔다");
        std::env::remove_var("CLAUDECODE");
        std::env::remove_var("CLAUDE_CODE_CHILD_SESSION");
        std::env::remove_var("CB_TEST_ONLY_PROCESS_VAR");
    }
}
