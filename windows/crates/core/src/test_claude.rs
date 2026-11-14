//! 시험용 claude 바꿔치기의 판정 — 보드(`app/src-tauri`)와 자율주행 `claude-autopilot start` 가 같은 규칙을 쓴다.
//!
//! 규칙(SPEC §5.13): 환경변수 `CLAUDE_BOARD_CLAUDE`(`|` 로 이은 절대 경로: 프로그램과 앞쪽 인자)가 있고
//! `CLAUDE_BOARD_HOME` 도 있을 때만 켜진다. `|` 로 가른 조각이 모두 비지 않은 절대 경로이며 실재해야 한다.
//! 홈을 따로 정한 실행에서만 켜지게 해서, 사용자 환경에 남은 변수 하나가 실수로 진짜 claude 를 바꿔치지 못하게 한다.
//!
//! 닫힌 채로 실패한다 — 꺼짐(Off) / 켜짐(On) / 어긋남(Rejected) 셋으로 가른다. 어긋남이면 진짜 claude 를 찾지도 띄우지도 않는다.
//! 판정만 둔다. 어긋남을 board.log 에 남기는 일은 보드의 몫이다.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::paths::HOME_ENV;

pub const CLAUDE_ENV: &str = "CLAUDE_BOARD_CLAUDE";

/// 판정 — 꺼짐 / 켜짐 / 켜려 했으나 어긋남(이유)
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
        return Decision::Rejected(format!("{CLAUDE_ENV} 는 {HOME_ENV} 도 있을 때만 쓴다. 무시한다"));
    }
    let Some(raw) = raw.to_str() else { return Decision::Rejected(format!("{CLAUDE_ENV} 가 유니코드가 아니다. 무시한다")) };
    let mut parts = Vec::new();
    for piece in raw.split('|') {
        if piece.is_empty() {
            return Decision::Rejected(format!("{CLAUDE_ENV} 에 빈 조각이 있다. 무시한다"));
        }
        if piece.contains('"') {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각에 큰따옴표가 있다({piece}). 무시한다"));
        }
        let path = PathBuf::from(piece);
        if !path.is_absolute() {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각이 절대 경로가 아니다({piece}). 무시한다"));
        }
        if !exists(&path) {
            return Decision::Rejected(format!("{CLAUDE_ENV} 조각이 없는 파일이다({piece}). 무시한다"));
        }
        parts.push(path);
    }
    Decision::On(parts)
}

/// 이 프로세스의 환경에서의 판정(파일은 실제로 있는지 본다).
pub fn decide_from_process() -> Decision {
    decide(&|key| std::env::var_os(key), &|p| p.exists())
}
