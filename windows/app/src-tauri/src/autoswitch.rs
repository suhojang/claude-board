//! 사용 한도 자동 전환 — `AutoSwitcher` (`ClaudeBoard.swift AutoSwitcher`, SPEC §5.6).
//!
//! 지금 계정의 5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 한도로 멈췄던 세션의
//! 콘솔에 이어서 진행하라는 요청을 넣는다. 다른 계정의 남은 한도는 미리 알 수 없어 그 계정이 지금 계정이었을 때
//! 한도가 찼던 기록으로 고른다. 결정마다 `notifier/auto-switch.log` 에 한 줄씩 남긴다.
//! 전환에 실패하면 실패의 쪽을 본다 — 전환하려던 계정만 건너뛰거나 잠시 쉬고, 지금 계정을 모를 때는 /login 안내를 알린다.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use claude_board_core::console::{self, Input};
use claude_board_core::{clock, paths};

use crate::accounts::{self, Account, SwitchSide};
use crate::board::Board;
use crate::model::{Limit, Session};
use crate::util::append_log;

/// 실행 중인 세션은 로그인 정보를 최대 30초 기억해 두고 쓴다 — 전환한 뒤 이만큼은 이전 계정으로 요청이 나갈 수 있다
pub const SETTLE: f64 = 40.0;
/// 이보다 오래된 한도 기록은 보지 않는다 — 오래전에 멈춘 세션을 계정을 바꿀 때마다 깨우지 않게
pub const RECENT: f64 = 6.0 * 3600.0;
pub const PROMPT: &str = "사용 한도로 멈췄던 세션을 다른 계정으로 전환해 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요.";
/// 찬 한도가 풀리는 시각을 모를 때, 전환에 실패한 계정을 다시 고르기까지. 지금 계정·시스템 쪽 실패로 자동 전환을 쉬는 기간도 이것
pub const RETRY_AFTER: f64 = 3600.0;
/// 쓰기 잠금을 잡지 못해 전환에 실패했을 때 자동 전환을 쉬는 기간 — Claude Code 가 로그인 정보를 쓰는 잠깐이라 짧게
pub const BUSY_RETRY: f64 = 30.0;

#[derive(Default)]
pub struct AutoSwitch {
    /// 자동 전환을 요청하고 결과를 기다리는 중
    pending: bool,
    /// 이어 가기를 넣은 "세션 id@한도 시각" — 한 번만 넣는다
    resumed: HashSet<String>,
    /// 전환할 계정이 없다고 이미 알렸다 — 계정이 바뀌기 전에는 다시 알리지 않는다
    no_candidate: bool,
    /// 전환 실패로 쉬는 계정·시각 — 그 계정이 지금 계정인 동안 전환·표시를 건드리지 않는다. 기억은 어플이 켜져 있는 동안만이다
    pause: Option<Pause>,
    /// 지금 계정을 알 수 없다고 이미 알렸다 — 로그인 정보가 돌아오기 전까지 한 번만 알린다
    unknown_current: bool,
}

impl AutoSwitch {
    /// 계정이 바뀌었다(손으로 바꾼 것 포함) — 다시 알릴 수 있게 하고, 전환 실패로 걸어둔 쉼도 접는다.
    pub fn switched(&mut self) {
        self.no_candidate = false;
        self.unknown_current = false;
        self.pause = None;
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

/// 찬 5시간·주간(전체) 한도 — 계정을 떠날 까닭과 표시를 채울 풀림 시각의 공통 조건(ClaudeBoard.swift blocksAccount).
/// 모델별 주간 한도는 그 모델을 쓰는 세션만 막으므로 계정을 떠나는 까닭·표시에 쓰지 않는다.
pub fn blocks(l: &Limit) -> bool {
    (l.id == "5h" || l.id == "7d") && l.pct >= 100.0
}

/// 지금 계정의 찬 5시간·주간 한도(풀리기 전).
pub fn full_limit(limits: &[Limit], now: f64) -> Option<&Limit> {
    limits.iter().find(|l| blocks(l) && l.resets.is_none_or(|r| r as f64 / 1000.0 > now))
}

fn key(s: &Session) -> String {
    format!("{}@{}", s.id, s.limit_s.unwrap_or(0.0))
}

/// 자동 전환 실패 뒤 잠시 쉬는 상태 — 그 계정이 지금 계정인 동안 전환·표시를 건드리지 않는다(§plan 3.3).
/// 기억은 어플이 켜져 있는 동안만이고 껐다 켜면 잊는다(ClaudeBoard.swift AutoSwitchPause).
pub struct Pause {
    pub account: String,
    pub until: f64,
}

/// 자동 전환의 판단 하나 — `decide` 가 내리는 결정 (§plan 3.3).
/// SwitchTo 가 Account 쌍을 통째로 옮겨 눈 하나에 256바이트를 넘긴다 — 계약이 정한 꼴을 그대로 두기 위해 허용한다.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Move {
    /// 지금 그대로 — 바꿀 까닭이 없다
    Stay,
    /// `from` 계정에서 `next` 계정으로 전환한다
    SwitchTo { next: Account, from: Account, reason: String, stopped: bool },
    /// 전환할 계정이 없다 — 이유는 이미 알렸다
    NoCandidate(String),
    /// 지금 계정을 알 수 없다 — 표시는 건드리지 않는다
    UnknownCurrent(String),
}

/// `decide` 의 결과 — 전환 결정과 한도 표시 갱신·이어 가기 목록.
pub struct Decision {
    /// 전환할 것인가
    pub mv: Move,
    /// 지금 계정의 찬 한도 표시를 갱신한 목록 — None 이면 건드리지 않는다
    pub full: Option<BTreeMap<String, f64>>,
    /// 이어 가기를 넣을 세션들
    pub resume: Vec<Session>,
}

/// 지금 상태에서 자동 전환 결정을 내린다 — 파일·프로세스를 만지지 않는 순수 판단(§plan 3.3).
/// 지금 계정을 모르면 떠날 까닭이 있을 때 UnknownCurrent 로 알린다 — 표시는 건드리지 않는다.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    sessions: &[Session],
    limits: &[Limit],
    current: Option<&Account>,
    saved: &[Account],
    full: &BTreeMap<String, f64>,
    resumed: &HashSet<String>,
    last_switch: f64,
    pending: bool,
    switching: bool,
    pause: Option<&Pause>,
    now: f64,
) -> Decision {
    let settled = last_switch + SETTLE;
    let limited = limited(sessions, now);
    // 이어 가기 — settle 이 지났으면 그 전(이전 계정)에 한도로 멈춘 세션, 아직 요청을 넣지 않은 것
    let resume = if now > settled {
        limited
            .iter()
            .filter(|s| s.limit_s.is_some_and(|at| at <= settled) && !resumed.contains(&key(s)))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    // 대기·전환 중·전환 직후(settle 안)에는 보지 않는다
    let reason = if pending || switching || now <= settled {
        None
    } else if let Some(hit) = limited.iter().find(|s| s.limit_s.is_some_and(|at| at > settled)) {
        Some(format!("{} 세션이 한도에 걸림({})", hit.project(), hit.message))
    } else {
        full_limit(limits, now).map(|l| format!("{} 한도 {}%", l.label, l.pct as i64))
    };
    // 쉬는 중 — 실패로 잠시 기다리는 계정이라 전환·표시를 건드리지 않는다. 지금 계정을 모르면 쉼의 끝 시각으로만 본다
    let resting = pause.is_some_and(|p| current.is_none_or(|c| p.account == c.id) && now < p.until);
    if resting {
        return Decision { mv: Move::Stay, full: None, resume };
    }
    let Some(reason) = reason else {
        return Decision { mv: Move::Stay, full: None, resume };
    };
    let Some(cur) = current else {
        // 떠날 까닭이 있는데 지금 계정을 모른다 — 알리고, 표시는 건드리지 않는다(ClaudeBoard.swift unknownCurrent)
        return Decision { mv: Move::UnknownCurrent(reason), full: None, resume };
    };
    // 표시 갱신 — 찬 5시간·주간 한도의 풀림 시각을 적는다(모르면 1시간 뒤). 만료된 표시는 지운다
    let resets = limits
        .iter()
        .filter(|l| blocks(l))
        .filter_map(|l| l.resets.map(|r| r as f64 / 1000.0))
        .filter(|r| *r > now)
        .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))));
    let mut marks = full.clone();
    marks.retain(|_, v| *v > now);
    marks.insert(cur.id.clone(), resets.unwrap_or(now + RETRY_AFTER));
    let stopped = sessions.iter().any(|s| s.limit_s.is_some() && s.state != "ended");
    let mv = match pick_next(saved, &cur.id, &marks, now).cloned() {
        Some(next) => Move::SwitchTo { next, from: cur.clone(), reason, stopped },
        None => Move::NoCandidate(reason),
    };
    Decision { mv, full: Some(marks), resume }
}

/// 전환이 실패했을 때의 표시와 쉼 — 실패의 쪽을 본다(ClaudeBoard.swift autoSwitchFailed, §plan 3.3).
/// 대상 계정 쪽이면 그 계정만 건너뛰고(다음 판단이 다음 후보를 고른다), 지금 계정·시스템 쪽이면 지금 계정이 잠시 쉰다.
pub fn failed(side: SwitchSide, target: &Account, current: &Account, full: &BTreeMap<String, f64>, now: f64) -> (BTreeMap<String, f64>, Option<Pause>) {
    match side {
        SwitchSide::Target => {
            let mut marks = full.clone();
            marks.insert(target.id.clone(), now + RETRY_AFTER);
            (marks, None)
        }
        SwitchSide::Other => (full.clone(), Some(Pause { account: current.id.clone(), until: now + RETRY_AFTER })),
        // 쓰기 잠금은 Claude Code 가 로그인 정보를 쓰는 순식간이니 짧게 쉰다
        SwitchSide::Busy => (full.clone(), Some(Pause { account: current.id.clone(), until: now + BUSY_RETRY })),
    }
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
    let sessions: Vec<Session> = board.sessions();
    let limits = board.usage.lock().unwrap().limits.clone();
    let (switching, current, saved) = {
        let acc = board.accounts.lock().unwrap();
        (acc.switching, acc.current.clone(), acc.saved.clone())
    };
    let marks = board.settings.lock().unwrap().auto_switch_full.clone();
    let decision = {
        let mut auto = board.auto.lock().unwrap();
        let decision = decide(&sessions, &limits, current.as_ref(), &saved, &marks, &auto.resumed, last, auto.pending, switching, auto.pause.as_ref(), now);
        // 지금 계정을 다시 알게 됐다 — 몰랐다는 알림의 문을 연다
        if current.is_some() {
            auto.unknown_current = false;
        }
        decision
    };

    // 1) 전환 — 결정을 옮긴다
    if let Some(full) = decision.full {
        board.update_settings(|st| st.auto_switch_full = full);
    }
    match decision.mv {
        Move::Stay => {}
        Move::UnknownCurrent(reason) => {
            // 한 번만 알린다 — 로그인 정보를 다시 읽어 계정이 정해지면 tick 이 문을 연다
            let mut notified = false;
            {
                let mut auto = board.auto.lock().unwrap();
                if !auto.unknown_current {
                    auto.unknown_current = true;
                    notified = true;
                }
            }
            if notified {
                log(&format!("{reason}: 지금 계정을 몰라 전환하지 못함"));
                board.flash("사용 한도에 도달했지만 지금 로그인된 계정의 정보를 읽지 못해 자동 전환을 하지 못했습니다. Claude Code 에서 /login 으로 다시 로그인해 주세요", true);
            }
        }
        Move::NoCandidate(reason) => {
            let mut notified = false;
            {
                let mut auto = board.auto.lock().unwrap();
                if !auto.no_candidate {
                    auto.no_candidate = true;
                    notified = true;
                }
            }
            if notified {
                log(&format!("{reason} — 전환할 계정 없음"));
                board.flash("사용 한도에 도달했지만 전환할 다른 계정이 없습니다 — 저장한 계정이 모두 한도에 도달했거나 다시 로그인이 필요합니다", true);
            }
        }
        Move::SwitchTo { next, from, reason, stopped } => {
            board.auto.lock().unwrap().pending = true;
            log(&format!("{reason} — {} 에서 {} 로 전환", from.email(), next.email()));
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
                Some(Box::new(move |r| {
                    b.auto.lock().unwrap().pending = false;
                    let Err(e) = r else { return };
                    let (marks, pause) = {
                        let full = b.update_settings(|st| st.auto_switch_full.clone());
                        failed(e.side, &failed_next, &from, &full, now)
                    };
                    b.update_settings(|st| st.auto_switch_full = marks);
                    b.auto.lock().unwrap().pause = pause;
                    match e.side {
                        SwitchSide::Target => log(&format!("{} 로 전환하지 못함 (1시간 동안 고르지 않음): {}", failed_next.email(), e.message)),
                        SwitchSide::Busy => log(&format!("{} 로 전환하지 못함, 30초 뒤 다시: {}", failed_next.email(), e.message)),
                        SwitchSide::Other => {
                            log(&format!("{} 로 전환하지 못함, 지금 계정이 바뀌거나 1시간이 지날 때까지 자동 전환을 쉼: {}", failed_next.email(), e.message));
                            b.flash(format!("사용 한도에 도달해 {} 계정으로 자동 전환하려 했지만 하지 못했습니다: {}. 계정이 바뀌거나 1시간이 지날 때까지 자동 전환을 쉽니다", failed_next.email(), e.message), true);
                        }
                    }
                })),
            );
        }
    }

    // 2) 이어 가기 — 결정의 세션에 이어서 진행하라는 요청을 넣는다
    let todo = decision.resume;
    if todo.is_empty() {
        return;
    }
    {
        let mut auto = board.auto.lock().unwrap();
        auto.resumed.extend(todo.iter().map(key));
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

// 시험 표 — T2 가 표 한 장으로 옮길 자리(§plan 3.3 C2). 지금은 비어 있다.
#[cfg(test)]
mod table;

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

    fn limited_session(id: &str, limit_s: f64) -> Session {
        use std::path::PathBuf;
        Session {
            id: id.into(),
            cwd: "C:/일".into(),
            state: "working".into(),
            since: 0,
            updated: 0,
            headless: false,
            title: String::new(),
            topic: String::new(),
            message: "작업 중".into(),
            name: None,
            ctx: None,
            brain: None,
            subagents: Vec::new(),
            shells: 0,
            paused: false,
            muted: false,
            limit_at: None,
            can_drive: true,
            place: crate::model::PidPlace::default(),
            pid: 7,
            pid_ctime: None,
            file: PathBuf::new(),
            project_dir: "일".into(),
            resume_args: Vec::new(),
            since_s: 0.0,
            updated_s: 0.0,
            limit_s: Some(limit_s),
        }
    }

    #[test]
    fn decide_unknown_current() {
        // 떠날 까닭이 없으면 그대로 — 표시도 건드리지 않는다
        let d = decide(&[], &[], None, &[], &BTreeMap::new(), &HashSet::new(), 0.0, false, false, None, 50.0);
        assert!(matches!(d.mv, Move::Stay), "{:?}", d.mv);
        assert!(d.full.is_none());
        assert!(d.resume.is_empty(), "settle 안에서는 이어 가기도 없다");

        // settle 뒤에는 이어 가기만 내놓는다
        let d = decide(&[limited_session("w1", 30.0)], &[], None, &[], &BTreeMap::new(), &HashSet::new(), 0.0, false, false, None, 50.0);
        assert!(matches!(d.mv, Move::Stay), "{:?}", d.mv);
        assert_eq!(d.resume.len(), 1);
        assert_eq!(d.resume[0].id, "w1");

        // 떠날 까닭(한도)이 있는데 지금 계정을 모른다 — UnknownCurrent 로 알리고 표시는 그대로둔다
        let limits = vec![limit("5h", 100.0, None)];
        let d = decide(&[], &limits, None, &[], &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 1000.0);
        let Move::UnknownCurrent(reason) = &d.mv else { panic!("알림 결정이 아니다: {:?}", d.mv) };
        assert_eq!(reason, "5h 한도 100%");
        assert!(d.full.is_none(), "지금 계정을 모르면 표시를 바꾸지 않는다");

        // 세션이 한도에 걸려 떠날 때도 같다
        let hit = limited_session("w1", 50.0);
        let d = decide(&[hit], &[], None, &[], &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 100.0);
        assert!(matches!(d.mv, Move::UnknownCurrent(_)), "{:?}", d.mv);
        assert!(d.full.is_none());
    }

    #[test]
    fn pause_blocks_switching_keeps_resume() {
        // 지금 계정이 쉬는 중이면 전환도 표시도 없다 — 이어 가기는 그대로 내놓는다
        let cur = acc("cur", None);
        let limits = vec![limit("5h", 100.0, None)];
        let pause = Pause { account: "cur|".into(), until: 1100.0 };
        let s = limited_session("w1", -70.0); // settled(-60) 이전에 걸린 세션 — 이어 가기 후보
        let d = decide(&[s], &limits, Some(&cur), &[], &BTreeMap::new(), &HashSet::new(), -100.0, false, false, Some(&pause), 1000.0);
        assert!(matches!(d.mv, Move::Stay), "{:?}", d.mv);
        assert!(d.full.is_none(), "쉬는 동안은 표시도 바꾸지 않는다");
        assert_eq!(d.resume.len(), 1);
        assert_eq!(d.resume[0].id, "w1");

        // 쉬는 계정이 지금 계정이 아니면 그대로 판단한다
        let other = Pause { account: "zz|".into(), until: 1100.0 };
        let saved = vec![acc("next", None)];
        let d = decide(&[], &limits, Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, Some(&other), 1000.0);
        assert!(matches!(d.mv, Move::SwitchTo { .. }), "{:?}", d.mv);
    }

    #[test]
    fn pause_ends_at_until() {
        // until 이 되면 다시 판단한다 — hold 는 now < until 이다
        let cur = acc("cur", None);
        let limits = vec![limit("5h", 100.0, None)];
        let saved = vec![acc("next", None)];
        let pause = Pause { account: "cur|".into(), until: 1000.0 };
        let d = decide(&[], &limits, Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, Some(&pause), 1000.0);
        assert!(matches!(d.mv, Move::SwitchTo { .. }), "{:?}", d.mv);
    }

    #[test]
    fn unknown_current_active_pause_stays() {
        // 지금 계정을 모르면 쉬기 시작한 계정과 견줄 수 없다 — 쉬는 중을 끝 시각으로만 본다
        let limits = vec![limit("5h", 100.0, None)];
        let pause = Pause { account: "gone|".into(), until: 1100.0 };
        let d = decide(&[], &limits, None, &[], &BTreeMap::new(), &HashSet::new(), -100.0, false, false, Some(&pause), 1000.0);
        assert!(matches!(d.mv, Move::Stay), "{:?}", d.mv);
        assert!(d.full.is_none());

        // 쉼이 끝났고 떠날 까닭이 있으면 UnknownCurrent 로 알린다
        let expired = Pause { account: "gone|".into(), until: 900.0 };
        let d = decide(&[], &limits, None, &[], &BTreeMap::new(), &HashSet::new(), -100.0, false, false, Some(&expired), 1000.0);
        assert!(matches!(d.mv, Move::UnknownCurrent(_)), "{:?}", d.mv);
    }

    #[test]
    fn marks_use_only_full_limits() {
        // 표시는 5시간·주간(전체) 한도만 본다 — 모델별 주간은 그 모델을 쓰는 세션만 막으므로 표시에 쓰지 않는다
        let cur = acc("cur", None);
        let saved = vec![acc("next", None)];
        let hit = limited_session("w1", 50.0); // settled(-60) 뒤에 걸림 — 떠날 까닭
        // 모델별 주간만 찼다 — 풀리는 시각을 모르는 것으로 1시간 뒤까지 표시한다
        let model_only = vec![limit("7d-Fable", 120.0, Some(5_000_000))];
        let d = decide(&[hit], &model_only, Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 1000.0);
        assert!(matches!(d.mv, Move::SwitchTo { .. }), "{:?}", d.mv);
        let marks = d.full.expect("표시를 바꿨어야 한다");
        assert_eq!(marks.get("cur|"), Some(&(1000.0 + 3600.0)), "모델별 주간 풀림을 쓰면 안 된다");

        // 5h·7d 이면 그 가장 늦은 풀림을 쓰고, 찬 모델별 주간 풀림은 무시한다
        let limits = vec![limit("7d", 100.0, Some(2_000_000)), limit("5h", 100.0, Some(1_500_000)), limit("7d-Fable", 150.0, Some(9_999_999))];
        let d = decide(&[], &limits, Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 1000.0);
        let marks = d.full.unwrap();
        assert_eq!(marks.get("cur|"), Some(&2000.0), "모델별 주간 풀림을 쓰면 안 된다");
    }

    #[test]
    fn decide_switches_with_marks() {
        // 5h 한도가 찼다 — 지금 계정에서 다음 계정으로 전환을 내놓고 지금 계정의 한도 표시를 채운다
        let limits = vec![limit("5h", 100.0, None)];
        let cur = acc("cur", None);
        let saved = vec![cur.clone(), acc("next", None)];
        let d = decide(&[], &limits, Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 1000.0);
        let Move::SwitchTo { next, from, reason, stopped } = &d.mv else { panic!("전환 결정이 아니다: {:?}", d.mv) };
        assert_eq!(next.id, "next|");
        assert_eq!(from.id, "cur|");
        assert_eq!(reason, "5h 한도 100%");
        assert!(!stopped);
        let marks = d.full.clone().unwrap();
        assert_eq!(marks.get("cur|"), Some(&(1000.0 + 3600.0))); // 풀리는 시각을 모르면 1시간 뒤

        // 전환할 계정이 없으면 없다고 본다
        let only = vec![cur.clone()];
        let d = decide(&[], &limits, Some(&cur), &only, &BTreeMap::new(), &HashSet::new(), -100.0, false, false, None, 1000.0);
        let Move::NoCandidate(reason) = &d.mv else { panic!("없음 결정이 아니다: {:?}", d.mv) };
        assert_eq!(reason, "5h 한도 100%");
        assert!(d.full.is_some());
    }

    #[test]
    fn decide_switches_when_session_hits_limit() {
        // settle 뒤에 새로 걸린 세션이 있다 — 멈춘 세션이 있으니 전환 안내에 이어 진행 표시도 붙는다
        let cur = acc("cur", None);
        let saved = vec![cur.clone(), acc("next", None)];
        let hit = limited_session("w1", 50.0);
        let d = decide(&[hit], &[], Some(&cur), &saved, &BTreeMap::new(), &HashSet::new(), 0.0, false, false, None, 100.0);
        let Move::SwitchTo { reason, stopped, .. } = &d.mv else { panic!("전환 결정이 아니다: {:?}", d.mv) };
        assert!(reason.contains("세션이 한도에 걸림(작업 중)"), "{reason}");
        assert!(stopped);
    }

    #[test]
    fn switched_clears_pause_and_unknown_current() {
        // 계정이 바뀌면(손으로 바꾼 것 포함) 쉼과 몰랐다는 알림의 문을 모두 푼다
        let mut auto = AutoSwitch { pause: Some(Pause { account: "cur|".into(), until: 9999.0 }), unknown_current: true, no_candidate: true, ..Default::default() };
        auto.switched();
        assert!(auto.pause.is_none());
        assert!(!auto.unknown_current);
        assert!(!auto.no_candidate);
    }

    #[test]
    fn failed_by_side() {
        // 실패의 쪽에 따라 표시와 쉼이 갈린다(plan 1 #5)
        let target = acc("t", None);
        let cur = acc("cur", None);
        let full = BTreeMap::from([("cur|".to_string(), 500.0)]);
        // 대상 계정 쪽 — 그 계정만 1시간 뒤까지 건너뛰고 쉬지 않는다(다음 판단이 다음 후보를 고른다)
        let (marks, pause) = failed(SwitchSide::Target, &target, &cur, &full, 1000.0);
        assert_eq!(marks.get("t|"), Some(&(1000.0 + 3600.0)));
        assert_eq!(marks.get("cur|"), Some(&500.0));
        assert!(pause.is_none());
        // 지금 계정·시스템 쪽 — 표시는 그대로, 지금 계정이 1시간 쉰다
        let (marks, pause) = failed(SwitchSide::Other, &target, &cur, &full, 1000.0);
        assert_eq!(marks.get("t|"), None, "표시는 그대로");
        assert_eq!(marks.get("cur|"), Some(&500.0));
        let p = pause.expect("지금 계정이 쉰다");
        assert_eq!(p.account, "cur|");
        assert_eq!(p.until, 1000.0 + 3600.0);
        // 쓰기 잠금 시간 초과 — 표시는 그대로, 30초만 쉰다
        let (marks, pause) = failed(SwitchSide::Busy, &target, &cur, &full, 1000.0);
        assert_eq!(marks.get("t|"), None, "표시는 그대로");
        let p = pause.expect("30초 쉰다");
        assert_eq!(p.account, "cur|");
        assert_eq!(p.until, 1000.0 + 30.0);
    }
}
