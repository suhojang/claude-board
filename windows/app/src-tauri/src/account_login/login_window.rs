//! 로그인 창 — 로그인 주소를 보드가 쥔 창으로 연다(mac `LoginWebWindow`). 창을 닫으면(로그인이 끝나기 전) 계정 추가를 취소한다.
//! 쿠키는 창마다 새로(incognito) — 브라우저에 로그인된 계정이 저절로 이어져 다른 계정을 못 넣는 일이 없게 한다.
//! 이 창은 `capabilities` 에 없는 라벨이라 페이지가 보드 명령(invoke)을 부를 수 없다. 한계: Google 은 앱 안 웹 창의 Google 로그인을 막을 수 있다
//! (이메일 로그인은 된다) — 그때는 설정 화면의 "다른 브라우저에서 열기"로 연다.
//!
//! 창을 만드는 곳이라 GUI 가 필요하다. 주소 검사(`login_url`)만 시험한다 — unchecked: 실제 창.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::util::board_log;
use crate::window;

const LABEL: &str = "account-login";

/// 열어도 되는 주소 — 로그인 명령 출력에서 읽은 https 주소만. 그 밖은 None.
pub fn login_url(text: &str) -> Option<Url> {
    let url = Url::parse(text).ok()?;
    (url.scheme() == "https" && url.host_str().is_some_and(|host| !host.is_empty())).then_some(url)
}

/// 지금 떠 있는 로그인 창 하나.
pub struct LoginWindow {
    app: AppHandle,
    current: Mutex<Option<(WebviewWindow, Arc<AtomicBool>)>>,
}

impl LoginWindow {
    pub fn new(app: AppHandle) -> Self {
        LoginWindow { app, current: Mutex::new(None) }
    }

    /// 창을 연다(이미 있으면 닫고 다시). 사용자가 닫으면 `on_user_closed` 를 부른다 — `close()` 로 닫은 것은 부르지 않는다.
    pub fn open(&self, text: &str, on_user_closed: impl Fn() + Send + 'static) {
        let Some(url) = login_url(text) else {
            board_log("로그인 주소가 https 가 아니라 창을 열지 않았습니다");
            return;
        };
        self.close();
        let pinned = window::main_window(&self.app).and_then(|w| w.is_always_on_top().ok()).unwrap_or(false);
        let built = WebviewWindowBuilder::new(&self.app, LABEL, WebviewUrl::External(url))
            .title("Claude 계정 추가")
            .inner_size(520.0, 720.0)
            .center()
            .always_on_top(pinned)
            .incognito(true)
            .build();
        let win = match built {
            Ok(win) => win,
            Err(e) => {
                board_log(&format!("로그인 창을 열지 못했습니다: {e}"));
                return;
            }
        };
        let ours = Arc::new(AtomicBool::new(false));
        let flag = ours.clone();
        win.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) && !flag.load(Ordering::SeqCst) {
                on_user_closed();
            }
        });
        *self.current.lock().unwrap() = Some((win, ours));
    }

    /// 로그인 창을 닫는다(없으면 아무 일도 없다). 사용자가 닫은 것으로 치지 않는다.
    pub fn close(&self) {
        if let Some((win, ours)) = self.current.lock().unwrap().take() {
            ours.store(true, Ordering::SeqCst);
            let _ = win.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_with_a_host_open() {
        assert!(login_url("https://claude.ai/oauth/authorize?x=1").is_some());
        assert!(login_url("http://claude.ai/oauth/authorize").is_none());
        assert!(login_url("file:///C:/Windows/notepad.exe").is_none());
        assert!(login_url("javascript:alert(1)").is_none());
        assert!(login_url("https://").is_none());
        assert!(login_url("not a url").is_none());
    }
}
