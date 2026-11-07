//! 모델 세션에 늘 거는 거부 목록 — 하네스 자리·사용자 설정·로그인 정보·시작 프로그램 폴더·`git push` (순수 함수).
//!
//! 네이티브 Windows 에는 Claude Code 샌드박스가 없어 권한 규칙이 유일한 벽이다. macOS 엔진의 `alwaysDenied`(파일 도구와 Bash 규칙)와
//! `harnessEntries`(샌드박스 denyWrite 가 막는 하네스 자리)를 같은 이름으로 옮기고, Windows 에서 같은 일을 하는 자리를 더했다.
//! 규칙 문안은 `SessionPolicy` 칸(`deny_write`·`deny_read`·`disallowed_tools`)으로 넘기고, 경로 → 규칙 꼴 변환은 `ExecEnv::render_settings` 가 한다.
//!
//! 정직한 한계: 이 규칙은 파일 도구(Edit·Write·Read)만 막는다. 네이티브 Windows 의 Bash 는 규칙 밖 경로로도 쓸 수 있다(샌드박스 없음).
//! 그래서 `git push` 는 명령 규칙으로 막지만 `cmd /c git push` 같은 우회는 막지 못한다.
//!
//! 같은 까닭으로 허용 도구 기본값도 환경에 따른다(`turn.rs` 의 `allowed_tools_base`): 고정 설정에 `allowed_tools` 가 없으면
//! 샌드박스가 있는 WSL 은 원본 기본 목록(맨 `Bash` 포함), 네이티브 Windows 는 맨 `Bash` 를 뺀 목록이다.
//! 네이티브에서 셸을 쓰려면 고정 설정이 `Bash(<접두> *)` 꼴 규칙을 명시해야 한다. 명시한 목록은 어느 환경에서나 그대로 쓴다.

use std::path::{Path, PathBuf};

/// 쓰기 범위 폴더마다 그 아래 하네스 자리 — 저장소 맨 위(macOS `harnessEntries` 와 같다).
/// `.claude` 는 통째로 막는다(파일 도구의 `Edit(.claude/**)` 와 같다). `.git` 쪽은 commit·worktree add 에 필요한
/// objects·refs·index·HEAD·logs 를 남기고 설정·훅·info 만 막는다
const HARNESS_TOP: &[&str] = &[
    "CLAUDE.md",
    "CLAUDE.local.md",
    ".mcp.json",
    ".gitattributes",
    ".gitmodules",
    ".claude",
    ".git/config",
    ".git/config.worktree",
    ".git/hooks",
    ".git/info",
    ".git/commondir",
    ".git/objects/info/alternates",
    ".git/worktrees/*/config.worktree",
    ".git/modules/*/hooks",
    ".git/modules/*/config",
    ".git/modules/*/config.worktree",
    ".git/modules/*/info",
    ".git/modules/*/objects/info/alternates",
];

/// 하위 저장소·submodule 의 하네스 자리 — 어느 깊이든(`**/`). macOS 는 깊이 4 까지 훑어 폴더마다 붙이지만 여기는 규칙 하나가 모든 깊이를 덮는다
const HARNESS_NESTED: &[&str] = &[
    "CLAUDE.md",
    "CLAUDE.local.md",
    ".mcp.json",
    ".gitattributes",
    ".gitmodules",
    ".claude/rules/**",
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".claude/skills/**",
    ".claude/agents/**",
    ".claude/commands/**",
    ".claude/hooks/**",
    ".claude/workflows/**",
    ".claude/output-styles/**",
    ".git/config",
    ".git/hooks/**",
    ".git/info/**",
];

/// 경로 없이(작업 폴더·홈 기준으로) 쓰는 거부 규칙. 환경과 상관없이 모든 세션에 건다 —
/// macOS `alwaysDenied` 의 같은 줄과 Windows 에서 더한 줄
const ALWAYS_DENIED: &[&str] = &[
    // 파일 도구(Edit 규칙은 Write 도구도 막는다)
    "Edit(**/CLAUDE.md)",
    "Edit(**/CLAUDE.local.md)",
    "Edit(**/.mcp.json)",
    "Edit(~/.claude/CLAUDE.md)",
    "Edit(.claude/**)",
    "Edit(**/.claude/rules/**)",
    "Edit(~/.claude/settings.json)",
    "Edit(~/.claude/settings.local.json)",
    "Edit(~/.claude.json)",
    "Read(~/.claude/.credentials.json)",
    "Edit(~/.claude/.credentials.json)",
    // 원격으로 내보내는 길(macOS 와 같은 세 꼴 + Windows 실행 파일 이름)
    "Bash(git push)",
    "Bash(git push *)",
    "Bash(git -C * push *)",
    "Bash(git.exe push)",
    "Bash(git.exe push *)",
    "Bash(git.exe -C * push *)",
    // macOS 의 `security`(키체인)에 대응하는 Windows 자격 증명 관리자 명령
    "Bash(cmdkey *)",
    "Bash(cmdkey.exe *)",
    "Bash(vaultcmd *)",
    "Bash(vaultcmd.exe *)",
    // 연결된 외부 서비스 쓰기(macOS 와 같다)
    "mcp__atlassian",
    "mcp__plugin_atlassian_atlassian",
    "mcp__claude_ai_Atlassian",
];

/// 사용자 영역(홈의 Claude Code 설정·로그인 정보·시작 프로그램). 네이티브 Windows 에서만 절대 경로로 건다
#[derive(Debug, Clone)]
pub struct UserArea {
    /// Claude Code 설정 폴더들(`~\.claude`, `CLAUDE_CONFIG_DIR` 가 있으면 그 폴더도)
    pub claude_dirs: Vec<PathBuf>,
    /// `~\.claude.json`
    pub claude_json: PathBuf,
    /// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
    pub startup: PathBuf,
}

impl UserArea {
    /// 이 프로세스의 환경에서. `CLAUDE_BOARD_HOME` 이 홈을 바꿨으면 `%APPDATA%` 도 그 아래로 본다(시험이 실제 폴더를 가리키지 않게)
    pub fn from_env() -> Self {
        let home = claude_board_core::paths::home();
        let mut claude_dirs = vec![claude_board_core::paths::claude_dir()];
        if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from) {
            if !claude_dirs.contains(&dir) {
                claude_dirs.push(dir);
            }
        }
        Self { claude_dirs, claude_json: claude_board_core::paths::claude_json(), startup: startup_dir(&home) }
    }
}

/// 시작 프로그램 폴더. `CLAUDE_BOARD_HOME` 이 홈을 바꿨으면 그 아래 `AppData\Roaming`, 아니면 `%APPDATA%`
pub fn startup_dir(home: &Path) -> PathBuf {
    let appdata = if claude_board_core::paths::overridden() {
        home.join("AppData").join("Roaming")
    } else {
        std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
    };
    appdata.join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup")
}

/// 거부 목록 한 벌
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Guard {
    pub deny_write: Vec<PathBuf>,
    pub deny_read: Vec<PathBuf>,
    pub disallowed_tools: Vec<String>,
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// 쓰기 범위(`roots` = 작업 폴더 + `write_roots`)마다 하네스 자리와, 사용자 영역(`user` 가 Some 일 때)의 거부 목록.
/// `user` 는 네이티브 Windows 에서만 준다 — WSL 은 홈이 리눅스 쪽이라 `~/` 꼴 규칙(`ALWAYS_DENIED`)이 맡는다
pub fn build(roots: &[PathBuf], user: Option<&UserArea>) -> Guard {
    let mut g = Guard::default();
    for tool in ALWAYS_DENIED {
        push_unique(&mut g.disallowed_tools, (*tool).to_string());
    }
    for root in roots {
        for entry in HARNESS_TOP {
            push_unique(&mut g.deny_write, root.join(entry));
        }
        for entry in HARNESS_NESTED {
            push_unique(&mut g.deny_write, root.join("**").join(entry));
        }
    }
    if let Some(u) = user {
        for dir in &u.claude_dirs {
            let creds = dir.join(".credentials.json");
            push_unique(&mut g.deny_read, creds.clone());
            push_unique(&mut g.deny_write, creds);
            for entry in ["settings*.json", "CLAUDE.md"] {
                push_unique(&mut g.deny_write, dir.join(entry));
            }
        }
        push_unique(&mut g.deny_write, u.claude_json.clone());
        push_unique(&mut g.deny_write, u.startup.clone());
    }
    g
}
