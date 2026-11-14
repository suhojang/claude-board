// 모델 세션에 늘 거는 거부 목록(`engine::run::guard`) 시험 — 목록 자체(순수)와, 네이티브 환경이 권한 규칙으로 바꾼 결과.
// 엔진이 이 목록을 정책에 붙이는지는 `run_engine.rs` 의 `session_policy_carries_the_always_on_deny_list` 가 본다.

use std::path::PathBuf;

use claude_board_autopilot::adapters::native_env::NativeEnv;
use claude_board_autopilot::engine::run::guard::{build, startup_dir, UserArea};
use claude_board_autopilot::ports::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn user() -> UserArea {
    UserArea {
        claude_dirs: vec![p(r"C:\Users\a\.claude")],
        claude_json: p(r"C:\Users\a\.claude.json"),
        startup: p(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup"),
    }
}

fn rendered(roots: &[PathBuf], user: Option<&UserArea>) -> (Vec<String>, Vec<String>) {
    let g = build(roots, user);
    let policy = SessionPolicy {
        allow_write: roots.to_vec(),
        deny_write: g.deny_write,
        deny_read: g.deny_read,
        disallowed_tools: g.disallowed_tools,
        ..SessionPolicy::default()
    };
    let env = NativeEnv::new(EnvChoice { kind: EnvKind::Pwsh, distro: None }, p(r"C:\x\pwsh.exe")).unwrap();
    let settings = env.render_settings(&policy).unwrap();
    let list = |k: &str| settings["permissions"][k].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    (list("allow"), list("deny"))
}

#[test]
fn every_root_gets_the_harness_spots_denied_for_file_tools() {
    let (_, deny) = rendered(&[p(r"C:\work\toy"), p(r"D:\shared")], Some(&user()));
    for root in ["//c/work/toy", "//d/shared"] {
        for want in [
            "Edit(//X/CLAUDE.md)",
            "Edit(//X/CLAUDE.local.md)",
            "Edit(//X/.mcp.json)",
            "Edit(//X/.gitattributes)",
            "Edit(//X/.gitmodules)",
            "Edit(//X/.claude)",
            "Edit(//X/.claude/**)",
            "Edit(//X/.git/config)",
            "Edit(//X/.git/config.worktree)",
            "Edit(//X/.git/hooks)",
            "Edit(//X/.git/hooks/**)",
            "Edit(//X/.git/info)",
            "Edit(//X/.git/info/**)",
            "Edit(//X/.git/worktrees/*/config.worktree)",
            "Edit(//X/.git/modules/*/hooks)",
            "Edit(//X/.git/modules/*/config)",
            "Edit(//X/**/CLAUDE.md)",
            "Edit(//X/**/CLAUDE.local.md)",
            "Edit(//X/**/.mcp.json)",
            "Edit(//X/**/.claude/rules/**)",
            "Edit(//X/**/.claude/settings.json)",
            "Edit(//X/**/.git/hooks/**)",
        ] {
            let rule = want.replace("//X", root);
            assert!(deny.contains(&rule), "{rule} 가 없다");
        }
    }
}

#[test]
fn user_area_credentials_settings_and_startup_are_denied() {
    let (_, deny) = rendered(&[p(r"C:\work\toy")], Some(&user()));
    for want in [
        // 로그인 정보는 읽기도 쓰기도 막는다
        "Read(//c/Users/a/.claude/.credentials.json)",
        "Edit(//c/Users/a/.claude/.credentials.json)",
        "Edit(//c/Users/a/.claude/settings*.json)",
        "Edit(//c/Users/a/.claude/CLAUDE.md)",
        "Edit(//c/Users/a/.claude.json)",
        "Edit(//c/Users/a/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup)",
        "Edit(//c/Users/a/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/**)",
    ] {
        assert!(deny.contains(&want.to_string()), "{want} 가 없다:\n{}", deny.join("\n"));
    }
}

#[test]
fn macos_always_denied_rules_and_git_push_are_carried_over() {
    let (_, deny) = rendered(&[p(r"C:\work\toy")], Some(&user()));
    for want in [
        "Edit(**/CLAUDE.md)",
        "Edit(~/.claude/CLAUDE.md)",
        "Edit(.claude/**)",
        "Edit(**/.claude/rules/**)",
        "Edit(~/.claude/settings.json)",
        "Edit(~/.claude/settings.local.json)",
        "Edit(~/.claude.json)",
        "Read(~/.claude/.credentials.json)",
        "Bash(git push)",
        "Bash(git push *)",
        "Bash(git -C * push *)",
        "Bash(git.exe push *)",
        "Bash(cmdkey *)",
        "mcp__atlassian",
        "mcp__plugin_atlassian_atlassian",
        "mcp__claude_ai_Atlassian",
    ] {
        assert!(deny.contains(&want.to_string()), "{want} 가 없다");
    }
}

#[test]
fn without_a_user_area_only_home_relative_rules_cover_the_user_files() {
    let g = build(&[p(r"C:\work\toy")], None);
    assert!(g.deny_read.is_empty(), "사용자 영역이 없으면 절대 경로 읽기 금지도 없다: {:?}", g.deny_read);
    assert!(g.deny_write.iter().all(|d| d.starts_with(r"C:\work\toy")), "{:?}", g.deny_write);
    assert!(g.disallowed_tools.contains(&"Read(~/.claude/.credentials.json)".to_string()));
    assert!(g.disallowed_tools.contains(&"Bash(git push *)".to_string()));
}

#[test]
fn rules_are_not_duplicated_and_rendering_is_stable() {
    let roots = [p(r"C:\work\toy"), p(r"C:\work\toy")];
    let g = build(&roots, Some(&user()));
    let mut seen = std::collections::HashSet::new();
    assert!(g.deny_write.iter().all(|d| seen.insert(d.clone())), "deny_write 에 같은 경로가 둘");
    let mut seen = std::collections::HashSet::new();
    assert!(g.disallowed_tools.iter().all(|d| seen.insert(d.clone())));
    assert_eq!(build(&roots, Some(&user())), g);
}

#[test]
fn startup_dir_follows_the_overridden_home() {
    let dir = startup_dir(&p(r"C:\h"));
    let text = dir.to_string_lossy().replace('\\', "/");
    assert!(text.ends_with("Microsoft/Windows/Start Menu/Programs/Startup"), "{text}");
    // 홈을 CLAUDE_BOARD_HOME 으로 바꾸지 않은 프로세스에서는 %APPDATA% 를 따른다
    if std::env::var_os(claude_board_core::paths::HOME_ENV).is_none() {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            assert!(dir.starts_with(appdata));
        }
    }
}
