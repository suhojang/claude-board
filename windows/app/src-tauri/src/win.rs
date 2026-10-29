//! Windows 전용 도구 — 창 앞으로 가져오기·비활성 표시·패널 창 스타일·소리·DPAPI·Windows Terminal 탭 고르기.
//! DPAPI 는 이 파일의 Windows 시험, 패널 창 스타일은 CI 의 화면 확인으로 실기 확인했다. 창 앞으로 가져오기·소리·Windows Terminal 탭 고르기는 **Windows 실기 미확인**.
//! 다른 OS 에서는 아무것도 하지 않는다(금고만 평문 그대로 — 개발용).

/// 계정 금고에 넣을 바이트를 암호화한다(Windows: DPAPI 현재 사용자, 그 밖: 그대로).
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    sys::protect(plain)
}

pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    sys::unprotect(sealed)
}

/// 시스템 더블 클릭 시간(ms) — 다른 터미널 줄의 한 번 클릭을 이만큼 미룬다. 읽지 못하면 500.
#[tauri::command]
pub fn double_click_time() -> u32 {
    sys::double_click_time()
}

pub use sys::{bring_to_front, play_sound, process_window, select_wt_tab, window_pid};
// 패널 스타일·비활성 표시는 Windows 에서만 부른다
#[cfg(windows)]
pub use sys::{show_no_activate, style_notify_panel};

#[cfg(windows)]
mod sys {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{LocalFree, HWND, LPARAM, TRUE};
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, BringWindowToTop,
        GWL_EXSTYLE, GW_OWNER, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE,
        SW_SHOWNOACTIVATE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// DPAPI 결과를 복사하고 LocalFree 로 놓는다(빈 결과는 포인터가 null 일 수 있다).
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI 가 채운 버퍼와 길이
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData.cast()) };
        v
    }

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        let input = blob(plain);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: 입력 버퍼는 이 함수 동안 살아 있고, 출력은 take 가 놓는다
        let ok = unsafe { CryptProtectData(&input, null(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) } != 0;
        ok.then(|| unsafe { take(out) })
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        let input = blob(sealed);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: 위와 같다
        let ok = unsafe { CryptUnprotectData(&input, null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) } != 0;
        ok.then(|| unsafe { take(out) })
    }

    /// 창을 앞으로 — 최소화면 복원하고, 포그라운드 잠금에 막히면 포그라운드 스레드에 입력을 붙여 다시 시도한다.
    pub fn bring_to_front(hwnd: isize) {
        let h = hwnd as HWND;
        // SAFETY: 창 핸들 조작만 한다(잘못된 핸들이면 실패할 뿐이다)
        unsafe {
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            if SetForegroundWindow(h) != 0 {
                return;
            }
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, null_mut());
            let me = GetCurrentThreadId();
            if fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, TRUE) != 0 {
                BringWindowToTop(h);
                SetForegroundWindow(h);
                AttachThreadInput(me, fg_thread, 0);
            }
        }
    }

    /// 포커스를 가져가지 않고 보인다.
    pub fn show_no_activate(hwnd: isize) {
        // SAFETY: 창 표시만 바꾼다
        unsafe { ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE) };
    }

    /// 알림 패널: 눌러도 활성화되지 않고(WS_EX_NOACTIVATE) 작업 표시줄·Alt+Tab 에 나타나지 않으며(WS_EX_TOOLWINDOW) 늘 위.
    pub fn style_notify_panel(hwnd: isize) {
        let h = hwnd as HWND;
        // SAFETY: 우리 창의 확장 스타일만 바꾼다
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            let ex = (ex | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST) & !WS_EX_APPWINDOW;
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }

    /// 시스템 소리 별칭을 비동기로 — done = 알림음(없으면 별표), wait = 경고음. 실패는 무시.
    pub fn play_sound(kind: &str) {
        let aliases: &[&str] = match kind {
            "done" => &["SystemNotification", "SystemAsterisk"],
            "wait" => &["SystemExclamation"],
            _ => &[],
        };
        for a in aliases {
            let w = wide(a);
            // SAFETY: NUL 로 끝나는 별칭 이름
            if unsafe { PlaySoundW(w.as_ptr(), null_mut(), SND_ALIAS | SND_ASYNC | SND_NODEFAULT) } != 0 {
                return;
            }
        }
    }

    pub fn double_click_time() -> u32 {
        // SAFETY: 인자 없는 시스템 설정 조회
        match unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() } {
            0 => 500,
            ms => ms,
        }
    }

    pub fn window_pid(hwnd: isize) -> u32 {
        let mut pid = 0u32;
        // SAFETY: 창 핸들 조회
        unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
        pid
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: EnumWindows 에 넘긴 Vec
        let list = unsafe { &mut *(lparam as *mut Vec<(u32, isize)>) };
        if unsafe { IsWindowVisible(hwnd) } != 0 && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() {
            list.push((window_pid(hwnd as isize), hwnd as isize));
        }
        TRUE
    }

    /// 그 프로세스들 가운데 하나의 보이는 소유자 없는 최상위 창.
    pub fn process_window(pids: &[u32]) -> Option<isize> {
        let mut list: Vec<(u32, isize)> = Vec::new();
        // SAFETY: 콜백은 EnumWindows 안에서만 list 를 쓴다
        unsafe { EnumWindows(Some(collect), &mut list as *mut _ as LPARAM) };
        list.into_iter().find(|(p, _)| pids.contains(p)).map(|(_, h)| h)
    }

    /// Windows Terminal 창 안에서 이름이 `marker` 인 탭을 UI Automation 으로 찾아 고른다.
    pub fn select_wt_tab(hwnd: isize, marker: &str) -> bool {
        use windows::Win32::Foundation::HWND as WHWND;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        };
        use windows::Win32::UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationSelectionItemPattern, TreeScope_Descendants, UIA_SelectionItemPatternId,
            UIA_TabItemControlTypeId,
        };
        /// 이 호출이 연 COM 을 닫는다 — 인터페이스(아래 find 안의 값)가 모두 놓인 뒤에 놓인다
        struct Com(bool);
        impl Drop for Com {
            fn drop(&mut self) {
                if self.0 {
                    // SAFETY: 성공한 CoInitializeEx 와 짝을 맞춘다
                    unsafe { CoUninitialize() };
                }
            }
        }
        // 보드의 작업 스레드에서 불린다. 이미 다른 방식으로 열려 있으면(RPC_E_CHANGED_MODE) 그대로 쓰고 닫지 않는다
        // SAFETY: 이 스레드의 COM 초기화
        let _com = Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok());
        // 탭 항목을 모두 훑어 이름이 표지인 것을 고른다(속성 조건은 VARIANT 를 만들어야 해 직접 거른다)
        let find = || -> windows::core::Result<bool> {
            // SAFETY: COM 호출 — 받은 인터페이스만 쓰고, 모두 이 클로저 안에서 놓인다
            unsafe {
                let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
                let root = uia.ElementFromHandle(WHWND(hwnd as *mut _))?;
                let all = root.FindAll(TreeScope_Descendants, &uia.CreateTrueCondition()?)?;
                for i in 0..all.Length()? {
                    let e = all.GetElement(i)?;
                    if e.CurrentControlType()? == UIA_TabItemControlTypeId && e.CurrentName()? == marker {
                        let p: IUIAutomationSelectionItemPattern = e.GetCurrentPatternAs(UIA_SelectionItemPatternId)?;
                        p.Select()?;
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        };
        find().unwrap_or(false)
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        Some(plain.to_vec())
    }
    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }
    pub fn bring_to_front(_hwnd: isize) {}
    pub fn play_sound(_kind: &str) {}
    pub fn double_click_time() -> u32 {
        500
    }
    pub fn window_pid(_hwnd: isize) -> u32 {
        0
    }
    pub fn process_window(_pids: &[u32]) -> Option<isize> {
        None
    }
    pub fn select_wt_tab(_hwnd: isize, _marker: &str) -> bool {
        false
    }
}

/// 실제 Windows 에서만 — DPAPI(현재 사용자) 봉인·풀기. 다른 OS 의 금고는 평문 그대로라 볼 것이 없다.
#[cfg(all(test, windows))]
mod tests {
    use super::{protect, unprotect};
    use crate::util::contains;

    /// 실패 메시지용 — 앞 48바이트 16진수
    fn hex(b: &[u8]) -> String {
        b.iter().take(48).map(|x| format!("{x:02x}")).collect()
    }

    /// 봉인 → 풀기가 원문으로 돌아오는지. 봉인은 원문과 달라야 한다. 봉인한 바이트를 돌려준다.
    fn round_trip(plain: &[u8]) -> Vec<u8> {
        let sealed = protect(plain)
            .unwrap_or_else(|| panic!("protect 실패 — 원문 {}바이트 ({})", plain.len(), std::io::Error::last_os_error()));
        let back = unprotect(&sealed).unwrap_or_else(|| {
            let err = std::io::Error::last_os_error();
            panic!("unprotect 실패 — 원문 {}바이트, 봉인 {}바이트 {}… ({err})", plain.len(), sealed.len(), hex(&sealed))
        });
        assert_eq!(back, plain, "왕복 결과가 원문과 다르다 — 봉인 {}바이트 {}…", sealed.len(), hex(&sealed));
        assert_ne!(sealed, plain, "봉인한 바이트가 원문 그대로다");
        sealed
    }

    #[test]
    fn dpapi_round_trip_hides_plain() {
        let token = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-시험용-0123456789abcdef","refreshToken":"sk-ant-ort01-시험용-fedcba9876543210","expiresAt":1}}"#;
        let sealed = round_trip(token.as_bytes());
        assert!(sealed.len() > token.len(), "봉인({}바이트)이 원문({}바이트)보다 짧다", sealed.len(), token.len());
        for needle in [token, "accessToken", "sk-ant-oat01-", "fedcba9876543210"] {
            assert!(
                !contains(&sealed, needle.as_bytes()),
                "봉인에 원문 조각 {needle:?} 이 그대로 있다 — {}…",
                String::from_utf8_lossy(&sealed[..sealed.len().min(200)])
            );
        }
        // 모든 바이트 값, 64KiB
        let bytes: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();
        let sealed = round_trip(&bytes);
        assert!(!contains(&sealed, &bytes[..256]), "봉인에 원문 0..=255 가 그대로 있다");
    }

    #[test]
    fn dpapi_empty_round_trip() {
        let sealed = protect(b"").unwrap_or_else(|| panic!("빈 원문 protect 실패 ({})", std::io::Error::last_os_error()));
        assert!(!sealed.is_empty(), "빈 원문의 봉인이 비어 있다");
        let back = unprotect(&sealed);
        assert_eq!(back.as_deref(), Some(&b""[..]), "빈 원문 왕복 실패 — 봉인 {}바이트 {}…", sealed.len(), hex(&sealed));
    }

    #[test]
    fn dpapi_rejects_damaged() {
        assert_eq!(unprotect(b""), None, "빈 입력이 풀렸다");
        // 고정 씨앗 xorshift — 매번 같은 "무작위" 바이트
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut noise = |n: usize| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect()
        };
        for n in [1, 16, 300, 1000] {
            let junk = noise(n);
            assert_eq!(unprotect(&junk), None, "무작위 {n}바이트가 풀렸다 — {}…", hex(&junk));
        }
        // 원문을 길게 잡아 봉인의 가운데가 암호문에 오게 한다
        let plain = "계정 토큰 sk-ant-oat01-0123456789abcdef ".repeat(30);
        let sealed = round_trip(plain.as_bytes());
        let flip = |i: usize| {
            let mut s = sealed.clone();
            s[i] ^= 0x5a;
            s
        };
        let n = sealed.len();
        let damaged = [
            ("끝 1바이트 잘림", sealed[..n - 1].to_vec()),
            ("절반으로 잘림", sealed[..n / 2].to_vec()),
            ("앞 16바이트만", sealed[..16].to_vec()),
            ("가운데 1바이트 바뀜", flip(n / 2)),
            ("마지막 바이트 바뀜", flip(n - 1)),
        ];
        for (what, bytes) in damaged {
            assert_eq!(unprotect(&bytes), None, "망가진 봉인({what}, 원래 {n}바이트)이 풀렸다");
        }
    }
}
