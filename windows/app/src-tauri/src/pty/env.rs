//! 터미널 셸에 넘길 환경 블록.
//!
//! 보드 프로세스의 환경을 물려주지 않고 사용자 계정의 환경(레지스트리의 시스템·사용자 환경변수)에서 새로 만든다.
//! 보드가 낡은 PATH 로 떴거나 부모 Claude 세션의 `CLAUDECODE`·`CLAUDE_CODE_CHILD_SESSION` 을 물려받아도 셸에는 가지 않는다.

/// `이름=값\0…\0\0` 블록을 (이름, 값) 목록으로. `=C:` 처럼 이름이 `=` 로 시작하는 항목은 그 이름 그대로 둔다.
#[cfg(test)]
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

/// 이 사용자 계정의 환경 블록(끝의 이중 NUL 포함). 실패는 호출한 쪽이 터미널을 못 여는 오류로 올린다.
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
    Ok(block)
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
        assert!(!has("CLAUDECODE"), "CLAUDECODE 가 새어 들어갔다");
        assert!(!has("CLAUDE_CODE_CHILD_SESSION"), "CLAUDE_CODE_CHILD_SESSION 이 새어 들어갔다");
        assert!(!has("CB_TEST_ONLY_PROCESS_VAR"), "이 프로세스에만 있던 변수가 새어 들어갔다");
        std::env::remove_var("CLAUDECODE");
        std::env::remove_var("CLAUDE_CODE_CHILD_SESSION");
        std::env::remove_var("CB_TEST_ONLY_PROCESS_VAR");
    }
}
