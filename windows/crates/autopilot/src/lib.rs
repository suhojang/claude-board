//! Claude Board 자율주행 엔진(Windows). 설계 원본은 macOS `docs/자율주행/설계.md`, Windows 약속은 `windows/docs/SPEC.md`.
//! 층: rules · engine::decide · stream (순수) → ports (트레이트) → adapters (바깥) → engine::run · cli (앱 서비스).
//! 순수 층은 ports 를 부르지 않는다. 바깥과는 engine::run 과 cli 만 말한다.

pub mod cli;
pub mod model;
pub mod ports;
pub mod stream;

pub mod rules {
    pub mod branches;
    pub mod board_git;
    pub mod builtin_versions;
    pub mod carry;
    pub mod checks;
    pub mod connections;
    pub mod control;
    pub mod critique;
    pub mod draft;
    /// Windows 전용(실행 환경 → 동의·판정 호출 꼴). PORTED.json 의 windows_only 에 둔다
    pub mod env_policy;
    pub mod evidence;
    pub mod formats;
    pub mod harness;
    pub mod leftovers;
    pub mod links;
    pub mod mcp_read;
    pub mod meta;
    pub mod miss_digest;
    pub mod misses;
    pub mod multirun;
    pub mod new_repo;
    pub mod notify_rules;
    pub mod outbox;
    pub mod path_key;
    pub mod gitlinks;
    pub mod history;
    pub mod import;
    pub mod inbox;
    pub mod instruction;
    pub mod judge_changes;
    pub mod prep_git;
    pub mod promotion;
    pub mod prose;
    pub mod purge;
    pub mod reading;
    pub mod records;
    pub mod report;
    pub mod report_gate;
    pub mod revise;
    pub mod scenarios;
    pub mod sequence;
    pub mod skeleton;
    pub mod slot_table;
    pub mod stack;
    pub mod start_settings;
    pub mod stopline;
    pub mod takeover;
    pub mod tickets;
    pub mod unit_commits;
    pub mod versions;
}

pub mod engine {
    pub mod branches;
    pub mod claims;
    pub mod decide;
    pub mod gitlinks_guard;
    pub mod leftovers;
    pub mod new_repo;
    pub mod prep_git;
    pub mod run;
    pub mod session_limits;
    pub mod session_runner;
    pub mod worktree_run;
}

pub mod adapters {
    pub mod files;
    pub mod git;
    pub mod http;
    pub mod native_env;
    pub mod records_sqlite;
    pub mod secrets;
    pub mod supervisor;
    pub mod usage;
    pub mod versions;
    pub mod winproc;
    pub mod worktree;
    pub mod wsl_env;
}
