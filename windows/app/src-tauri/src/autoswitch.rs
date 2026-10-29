//! 사용 한도 자동 전환 — `AutoSwitcher` (`ClaudeBoard.swift AutoSwitcher`, SPEC §5.6).
//!
//! 지금 계정의 5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 한도로 멈췄던 세션의
//! 콘솔에 이어서 진행하라는 요청을 넣는다. 다른 계정의 남은 한도는 미리 알 수 없어 그 계정이 지금 계정이었을 때
//! 한도가 찼던 기록으로 고른다. 결정마다 `notifier/auto-switch.log` 에 한 줄씩 남긴다.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use claude_board_core::console::{self, Input};
use claude_board_core::{clock, paths};

use crate::accounts::{self, Account};
use crate::board::Board;
use crate::model::{Limit, Session};
use crate::util::append_log;

/// 실행 중인 세션은 로그인 정보를 최대 30초 기억해 두고 쓴다 — 전환한 뒤 이만큼은 이전 계정으로 요청이 나갈 수 있다
pub const SETTLE: f64 = 40.0;
/// 이보다 오래된 한도 기록은 보지 않는다 — 오래전에 멈춘 세션을 계정을 바꿀 때마다 깨우지 않게
pub const RECENT: f64 = 6.0 * 3600.0;
pub const PROMPT: &str = "사용 한도로 멈췄던 세션을 다른 계정으로 전환해 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요.";

#[derive(Default)]
pub struct AutoSwitch {
    /// 자동 전환을 요청하고 결과를 기다리는 중
    pending: bool,
    /// 이어 가기를 넣은 "세션 id@한도 시각" — 한 번만 넣는다
    resumed: HashSet<String>,
    /// 전환할 계정이 없다고 이미 알렸다 — 계정이 바뀌기 전에는 다시 알리지 않는다
    no_candidate: bool,
}

impl AutoSwitch {
    /// 계정이 바뀌었다(손으로 바꾼 것 포함) — 다시 알릴 수 있게 한다.
    pub fn switched(&mut self) {
        self.no_candidate = false;
    }
}

fn log(text: &str) {
    append_log(&paths::notifier_dir().join("auto-switch.log"), text);
}

/// 저장한 계정 가운데 지금 계정 다음부터 돌며 쓸 수 있는(다시 로그인 필요 아님, 한도 기록 없음) 첫 계정.
pub fn pick_next<'a>(saved: &'a [Account], cur: &str, marks: &BTreeMap<String, f64>, now: f64) -> Option<&'a Account> {
    let start = saved.iter().position(|a| a.id == cur).map_or(0, |i| i + 1);
    saved[start..].iter().chain(&saved[..start]).find(|a| a.id != cur && !a.expired(now) && !marks.contains_key(&a.id))
}

/// 한도에 걸려 멈춘 세션 — 종료·일시중지·승인 대기(Esc 가 승인을 거절한다)는 건드리지 않는다.
pub fn limited(sessions: &[Session], now: f64) -> Vec<Session> {
    sessions
        .iter()
        .filter(|s| !matches!(s.state.as_str(), "ended" | "paused" | "waiting") && s.limit_s.is_some_and(|at| now - at < RECENT))
        .cloned()
        .collect()
}

/// 지금 계정의 찬 5시간·주간 한도(풀리기 전).
pub fn full_limit(limits: &[Limit], now: f64) -> Option<&Limit> {
    limits.iter().find(|l| (l.id == "5h" || l.id == "7d") && l.pct >= 100.0 && l.resets.is_none_or(|r| r as f64 / 1000.0 > now))
}

fn key(s: &Session) -> String {
    format!("{}@{}", s.id, s.limit_s.unwrap_or(0.0))
}

/// 3초마다.
pub fn tick(board: &Arc<Board>) {
    let (on, last) = {
        let st = board.settings.lock().unwrap();
        (st.auto_switch, st.auto_switch_last)
    };
    if !on {
        return;
    }
    let now = clock::now();
    let settled = last + SETTLE;
    let sessions: Vec<Session> = board.sessions();
    let limited = limited(&sessions, now);
    let limits = board.usage.lock().unwrap().limits.clone();

    // 1) 전환 — 지금 계정으로 새로 한도에 걸린 세션이 있거나, 지금 계정의 5시간·주간 한도가 찼다
    let (switching, current) = {
        let acc = board.accounts.lock().unwrap();
        (acc.switching, acc.current.clone())
    };
    let pending = board.auto.lock().unwrap().pending;
    if let (false, false, true, Some(cur)) = (pending, switching, now > settled, current) {
        if let Some(hit) = limited.iter().find(|s| s.limit_s.is_some_and(|at| at > settled)) {
            switch_away(board, &cur, format!("{} 세션이 한도에 걸림({})", hit.project(), hit.message), &limits, now);
        } else if let Some(l) = full_limit(&limits, now) {
            switch_away(board, &cur, format!("{} 한도 {}%", l.label, l.pct as i64), &limits, now);
        }
    }

    // 2) 이어 가기 — 전환하고 settle 이 지났으면, 그 전(이전 계정)에 한도로 멈춘 세션에 이어서 진행하라는 요청을 넣는다
    if now <= settled {
        return;
    }
    let todo: Vec<Session> = {
        let mut auto = board.auto.lock().unwrap();
        let todo: Vec<Session> = limited.into_iter().filter(|s| s.limit_s.is_some_and(|at| at <= settled) && !auto.resumed.contains(&key(s))).collect();
        auto.resumed.extend(todo.iter().map(key));
        todo
    };
    if todo.is_empty() {
        return;
    }
    let mut manual = Vec::new();
    for s in &todo {
        let name = s.project();
        if !s.can_drive {
            log(&format!("{name} — 콘솔에 입력할 수 없는 세션이라 이어 가지 못함"));
            manual.push(name);
            continue;
        }
        let pid = s.pid;
        std::thread::spawn(move || {
            // Esc 를 먼저 — Claude Code 가 띄운 한도 선택 창을 닫고 "한도가 풀리면 저절로 이어 가기" 예약을 취소한다
            let inputs = [Input::Escape, Input::Pause(Duration::from_millis(500)), Input::Text(PROMPT), Input::Pause(Duration::from_millis(200)), Input::Enter];
            let ok = console::send_input(pid, &inputs);
            log(&format!("{name} — {}", if ok { "이어 가기 요청을 넣음" } else { "콘솔에 붙지 못해 이어 가지 못함" }));
        });
    }
    let n = todo.len() - manual.len();
    let mut text = if n > 0 { format!("한도로 멈췄던 세션 {n}개를 이어서 진행합니다") } else { String::new() };
    if !manual.is_empty() {
        text += &format!("{}{} 세션은 터미널에 입력할 수 없어 직접 이어 주세요", if n > 0 { " — " } else { "" }, manual.join(", "));
    }
    board.flash(text, !manual.is_empty());
}

/// 지금 계정은 한도가 풀릴 때까지 고르지 않게 적어 두고, 저장한 계정 가운데 다음으로 쓸 수 있는 계정으로 바꾼다.
fn switch_away(board: &Arc<Board>, cur: &Account, reason: String, limits: &[Limit], now: f64) {
    // 풀리는 시각은 지금 계정의 찬 한도에서 읽는다 — 모르면 1시간 뒤 다시 본다
    let resets = limits
        .iter()
        .filter(|l| l.pct >= 100.0)
        .filter_map(|l| l.resets.map(|r| r as f64 / 1000.0))
        .filter(|r| *r > now)
        .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))));
    let marks = board.update_settings(|st| {
        st.auto_switch_full.retain(|_, v| *v > now);
        st.auto_switch_full.insert(cur.id.clone(), resets.unwrap_or(now + 3600.0));
        st.auto_switch_full.clone()
    });
    let saved = board.accounts.lock().unwrap().saved.clone();
    let Some(next) = pick_next(&saved, &cur.id, &marks, now).cloned() else {
        let mut auto = board.auto.lock().unwrap();
        if !auto.no_candidate {
            auto.no_candidate = true;
            drop(auto);
            log(&format!("{reason} — 전환할 계정 없음"));
            board.flash("사용 한도에 도달했지만 전환할 다른 계정이 없습니다 — 저장한 계정이 모두 한도에 도달했거나 다시 로그인이 필요합니다", true);
        }
        return;
    };
    board.auto.lock().unwrap().pending = true;
    log(&format!("{reason} — {} 에서 {} 로 전환", cur.email(), next.email()));
    let stopped = board.sessions().iter().any(|s| s.limit_s.is_some() && s.state != "ended");
    let mut note = format!("사용 한도에 도달해 {} 계정으로 자동 전환했습니다", next.email());
    if stopped {
        note += &format!(" — 한도로 멈춘 세션은 {}초 뒤 이어서 진행합니다", SETTLE as i64);
    }
    let b = board.clone();
    let failed_next = next.clone();
    accounts::switch_to(
        board,
        next,
        Some(note),
        Some(Box::new(move |ok| {
            b.auto.lock().unwrap().pending = false;
            if !ok {
                log(&format!("{} 로 전환하지 못함 — 1시간 동안 고르지 않음", failed_next.email()));
                b.update_settings(|st| {
                    st.auto_switch_full.insert(failed_next.id.clone(), now + 3600.0);
                });
            }
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn acc(uuid: &str, exp: Option<f64>) -> Account {
        Account::new(json!({"accountUuid": uuid, "emailAddress": format!("{uuid}@x")}).as_object().unwrap(), exp).unwrap()
    }

    #[test]
    fn picks_next_usable_account() {
        let saved = vec![acc("a", None), acc("b", Some(10.0)), acc("c", None), acc("d", None)];
        let none = BTreeMap::new();
        // 지금 계정 다음부터 돈다 — b 는 다시 로그인 필요라 건너뛴다
        assert_eq!(pick_next(&saved, "a|", &none, 100.0).unwrap().id, "c|");
        assert_eq!(pick_next(&saved, "c|", &none, 100.0).unwrap().id, "d|");
        // 끝에서 처음으로 돌아간다
        assert_eq!(pick_next(&saved, "d|", &none, 100.0).unwrap().id, "a|");
        // 한도 기록이 있는 계정은 고르지 않는다
        let marks = BTreeMap::from([("d|".to_string(), 200.0), ("a|".to_string(), 200.0)]);
        assert_eq!(pick_next(&saved, "c|", &marks, 100.0), None);
        // 저장 목록에 없는 지금 계정이면 처음부터
        assert_eq!(pick_next(&saved, "zzz|", &marks, 100.0).unwrap().id, "c|");
        // b 가 아직 유효하면 고른다
        assert_eq!(pick_next(&saved, "a|", &none, 5.0).unwrap().id, "b|");
        assert_eq!(pick_next(&[], "a|", &none, 5.0), None);
    }

    fn limit(id: &str, pct: f64, resets: Option<i64>) -> Limit {
        Limit { id: id.into(), label: id.into(), short: id.into(), tiny: id.into(), pct, resets }
    }

    #[test]
    fn full_limits_and_limited_sessions() {
        let now = 1000.0;
        assert!(full_limit(&[limit("5h", 99.0, None), limit("7d-Fable", 100.0, None)], now).is_none());
        assert_eq!(full_limit(&[limit("5h", 100.0, Some(2_000_000))], now).unwrap().id, "5h");
        // 이미 풀린 한도는 보지 않는다
        assert!(full_limit(&[limit("7d", 100.0, Some(500_000))], now).is_none());
        assert_eq!(full_limit(&[limit("7d", 120.0, None)], now).unwrap().id, "7d");
    }
}
