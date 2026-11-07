//! Claude Board 자율주행 엔진(Windows). 설계 원본은 macOS `docs/자율주행/설계.md`, Windows 약속은 `windows/docs/SPEC.md`.
//! 층: rules · engine::decide · stream (순수) → ports (트레이트) → adapters (바깥) → engine::run · cli (앱 서비스).
//! 순수 층은 ports 를 부르지 않는다. 바깥과는 engine::run 과 cli 만 말한다.

pub mod cli;
pub mod model;
pub mod ports;
pub mod stream;

pub mod rules {
    pub mod checks;
    pub mod control;
    /// Windows 전용(실행 환경 → 동의·판정 호출 꼴). PORTED.json 의 windows_only 에 둔다
    pub mod env_policy;
    pub mod evidence;
    pub mod records;
    pub mod stopline;
}

pub mod engine {
    pub mod decide;
    pub mod run;
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
    pub mod winproc;
    pub mod wsl_env;
}
