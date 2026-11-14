//! 엔진 git 보호 시험 — macOS `EngineGit.swift` 의 늦은 받기·전송·서명 확인·bare 저장소·하위 모듈·replace 객체 보호가
//! 진짜 git.exe 에서 심은 코드를 막는지 본다. 모든 시험은 짝이 있다: 보호 없는 git 으로 같은 일을 하면 표식 파일이 생기는지(대조군)를
//! 먼저 보고, 어댑터(`ProcessGit`)로 하면 생기지 않는지 본다. 대조군이 서지 않으면 시험이 힘이 없으므로 실패한다.

use claude_board_autopilot::adapters::git::ProcessGit;
use claude_board_autopilot::ports::Git;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// 보호 없는 git. 프로세스 환경은 그대로 물려받는다(대조군 전용)
fn raw(repo: &Path, args: &[&str]) -> (i32, String, String) {
    let o = Command::new("git").arg("-C").arg(repo).args(args).output().expect("git 이 있어야 한다");
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn raw_ok(repo: &Path, args: &[&str]) -> String {
    let (code, out, err) = raw(repo, args);
    assert_eq!(code, 0, "{args:?}: {err}");
    out
}

/// 보호 git 으로 부른다(엔진이 부르는 길)
fn guarded(repo: &Path, args: &[&str]) -> (i32, String, String) {
    let o = ProcessGit::new().run(repo, args).expect("git 이 있어야 한다");
    (o.code, o.stdout, o.stderr)
}

fn init_with_commit(dir: &Path) {
    raw_ok(dir, &["init", "-q", "-b", "main"]);
    for (k, v) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        raw_ok(dir, &["config", k, v]);
    }
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    raw_ok(dir, &["add", "a.txt"]);
    raw_ok(dir, &["commit", "-q", "-m", "a"]);
}

fn marker_in(dir: &Path, name: &str) -> (PathBuf, String) {
    let m = dir.join(name);
    let s = slashes(&m);
    (m, s)
}

/// 커밋한 a.txt 의 blob 을 저장소에서 지워 부분 복제로 빠진 객체처럼 만든다(읽으면 늦은 받기를 시도)
fn remove_blob(repo: &Path) {
    let oid = raw_ok(repo, &["rev-parse", "HEAD:a.txt"]).trim().to_string();
    let loose = repo.join(".git").join("objects").join(&oid[..2]).join(&oid[2..]);
    let mut perm = std::fs::metadata(&loose).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&loose, perm).unwrap();
    std::fs::remove_file(&loose).unwrap();
}

fn make_promisor(repo: &Path, url: &str) {
    for (k, v) in [
        ("core.repositoryformatversion", "1"),
        ("extensions.partialclone", "origin"),
        ("remote.origin.url", url),
        ("remote.origin.promisor", "true"),
        ("remote.origin.partialclonefilter", "blob:none"),
    ] {
        raw_ok(repo, &["config", k, v]);
    }
}

/// 부분 복제 저장소에서 빠진 객체를 읽으면 git 이 원격 명령을 부른다 — core.sshCommand 로 심은 명령
#[test]
fn lazy_fetch_ssh_command_does_not_run() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_with_commit(repo);
    remove_blob(repo);
    make_promisor(repo, "ssh://planted.invalid/x");
    let (marker, path) = marker_in(repo, "marker-ssh");
    raw_ok(repo, &["config", "core.sshCommand", &format!("echo x > '{path}'; exit 1")]);

    // 대조군: 보호 없는 읽기가 심은 ssh 명령을 돌린다
    let _ = raw(repo, &["show", "HEAD:a.txt"]);
    assert!(marker.exists(), "보호 없는 git 이 늦은 받기로 core.sshCommand 를 돌리지 않았다 — 시험이 힘이 없다");
    std::fs::remove_file(&marker).unwrap();

    let (code, _, _) = guarded(repo, &["show", "HEAD:a.txt"]);
    assert_ne!(code, 0, "빠진 객체를 읽는 걸음은 실패해야 한다(닫힌 쪽)");
    assert!(!marker.exists(), "어댑터의 늦은 받기가 core.sshCommand 를 돌렸다");
}

/// 같은 일을 원격이 로컬 경로일 때 remote.<이름>.uploadpack 으로 심은 명령으로
#[test]
fn lazy_fetch_uploadpack_does_not_run() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    std::fs::create_dir_all(&repo).unwrap();
    init_with_commit(&repo);
    remove_blob(&repo);
    make_promisor(&repo, &slashes(&tmp.path().join("origin")));
    let (marker, path) = marker_in(tmp.path(), "marker-uploadpack");
    raw_ok(&repo, &["config", "remote.origin.uploadpack", &format!("echo x > '{path}'; exit 1")]);

    let _ = raw(&repo, &["show", "HEAD:a.txt"]);
    assert!(marker.exists(), "보호 없는 git 이 늦은 받기로 uploadpack 을 돌리지 않았다 — 시험이 힘이 없다");
    std::fs::remove_file(&marker).unwrap();

    let (code, _, _) = guarded(&repo, &["show", "HEAD:a.txt"]);
    assert_ne!(code, 0);
    assert!(!marker.exists(), "어댑터의 늦은 받기가 uploadpack 을 돌렸다");
}

/// git log 가 서명된 커밋을 만나면 저장소 설정의 gpg.program 을 부른다(log.showSignature=true)
#[test]
fn log_show_signature_does_not_run_gpg_program() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    std::fs::create_dir_all(&repo).unwrap();
    init_with_commit(&repo);
    let (marker, path) = marker_in(tmp.path(), "marker-gpg");
    let script = tmp.path().join("fake-gpg.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho x > '{path}'\nexit 1\n")).unwrap();
    raw_ok(&repo, &["config", "gpg.program", &slashes(&script)]);
    raw_ok(&repo, &["config", "log.showSignature", "true"]);

    // 서명 머리글이 있는 커밋을 직접 만든다(서명이 맞는지는 gpg.program 이 가린다)
    let body = raw_ok(&repo, &["cat-file", "commit", "HEAD"]);
    let signed = body.replacen(
        "\n\n",
        "\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n abc\n -----END PGP SIGNATURE-----\n\n",
        1,
    );
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(signed.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(oid.len(), 40, "서명 머리글 커밋을 못 만들었다");
    raw_ok(&repo, &["update-ref", "refs/heads/main", &oid]);

    let _ = raw(&repo, &["log", "-1"]);
    assert!(marker.exists(), "보호 없는 git log 가 gpg.program 을 돌리지 않았다 — 시험이 힘이 없다");
    std::fs::remove_file(&marker).unwrap();

    let (code, out, err) = guarded(&repo, &["log", "-1"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("commit "), "{out}");
    assert!(!marker.exists(), "어댑터의 git log 가 gpg.program 을 돌렸다");
}

/// 작업 폴더에 심은 중첩 git(gitlink)의 filter — status 가 하위 저장소로 들어가 돌린다
#[test]
fn nested_gitlink_filter_does_not_run_on_status() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    std::fs::create_dir_all(&repo).unwrap();
    init_with_commit(&repo);

    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    init_with_commit(&sub);
    let (marker, path) = marker_in(tmp.path(), "marker-gitlink");
    raw_ok(&sub, &["config", "filter.x.clean", &format!("echo x > '{path}'; cat")]);
    raw_ok(&sub, &["config", "filter.x.required", "false"]);
    std::fs::write(sub.join(".gitattributes"), "*.txt filter=x\n").unwrap();
    // 상위 저장소에 gitlink 로 올리고 하위 모듈로 등록한다(모델이 작업 폴더에 심을 수 있는 꼴)
    raw_ok(&repo, &["add", "sub"]);
    std::fs::write(repo.join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n\turl = ./sub\n").unwrap();
    raw_ok(&repo, &["config", "submodule.sub.url", "./sub"]);
    raw_ok(&repo, &["config", "submodule.active", "."]);
    // 같은 길이로 고쳐 git 이 내용을 다시 읽게 한다
    std::fs::write(sub.join("a.txt"), "uno\ntwo\n").unwrap();

    let _ = raw(&repo, &["status", "--porcelain"]);
    if !marker.exists() {
        // 이 git 판이 하위 저장소의 filter 를 상위 status 에서 돌리지 않으면 공격을 세울 수 없다 — 숨기지 않고 실패로 알린다
        panic!("대조군이 서지 않았다: 보호 없는 상위 status 가 하위 저장소 filter 를 돌리지 않았다");
    }
    std::fs::remove_file(&marker).unwrap();

    for args in [&["status", "--porcelain"][..], &["diff", "--stat"], &["switch", "main"]] {
        let (code, _, err) = guarded(&repo, args);
        assert_eq!(code, 0, "{args:?}: {err}");
    }
    assert!(!marker.exists(), "어댑터가 하위 저장소의 filter 를 돌렸다");
}

/// 작업 폴더 안의 bare 저장소 꼴 폴더를 git 이 스스로 저장소로 쓰지 않는다
#[test]
fn bare_shaped_directory_is_not_used_as_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let evil = work.join("evil");
    std::fs::create_dir_all(&evil).unwrap();
    raw_ok(&evil, &["init", "-q", "--bare"]);

    // 대조군: 보호 없는 git 은 그 폴더를 bare 저장소로 쓴다
    let (code, out, _) = raw(&evil, &["rev-parse", "--is-bare-repository"]);
    assert_eq!((code, out.trim()), (0, "true"), "보호 없는 git 이 bare 꼴 폴더를 저장소로 쓰지 않았다 — 시험이 힘이 없다");

    let (code, _, err) = guarded(&evil, &["rev-parse", "--is-bare-repository"]);
    assert_ne!(code, 0, "어댑터가 bare 꼴 폴더를 저장소로 썼다");
    assert!(err.contains("bare") || err.contains("safe.bareRepository"), "{err}");
}

/// replace 객체로 읽는 내용을 바꿔치지 못한다
#[test]
fn replace_refs_are_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_with_commit(repo);
    let old = raw_ok(repo, &["rev-parse", "HEAD:a.txt"]).trim().to_string();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"replaced\n").unwrap();
    let new = String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).trim().to_string();
    raw_ok(repo, &["replace", &old, &new]);

    assert_eq!(raw_ok(repo, &["cat-file", "-p", &old]), "replaced\n", "보호 없는 git 이 replace 를 따르지 않았다 — 시험이 힘이 없다");
    let (code, out, err) = guarded(repo, &["cat-file", "-p", &old]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.replace("\r\n", "\n"), "one\ntwo\n", "어댑터가 replace 객체를 따랐다");
}

/// 물려받은 GIT_CONFIG_PARAMETERS 로 심은 filter 가 돌지 않는다(환경에서 GIT_* 를 모두 뺀다)
#[test]
fn inherited_git_config_parameters_are_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_with_commit(repo);
    let (marker, path) = marker_in(repo, "marker-env");
    std::fs::write(repo.join(".gitattributes"), "*.txt filter=x\n").unwrap();
    std::fs::write(repo.join("a.txt"), "uno\ntwo\n").unwrap();
    let param = format!("'filter.x.clean=echo x > '\\''{path}'\\''; cat'");

    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain"])
        .env("GIT_CONFIG_PARAMETERS", &param)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(marker.exists(), "보호 없는 git 이 GIT_CONFIG_PARAMETERS 의 filter 를 돌리지 않았다 — 시험이 힘이 없다");
    std::fs::remove_file(&marker).unwrap();

    // 프로세스 환경에 심어야 "물려받음" 이 되는데, 이 프로세스의 환경을 바꾸면 병렬로 도는 다른 시험의 git 이 오염된다.
    // 그래서 시험 바이너리를 자식으로 다시 띄워 그 안에서만 환경을 갖게 한다(아래 `inherited_env_child`)
    let exe = std::env::current_exe().unwrap();
    let o = Command::new(exe)
        .args(["--exact", "inherited_env_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env("GIT_CONFIG_PARAMETERS", &param)
        .env("CB_CHILD_REPO", repo)
        .env("CB_CHILD_MARKER", &marker)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "자식 시험이 실패했다:\n{text}");
    assert!(text.contains("inherited_env_child ... ok"), "자식 시험이 실제로 돌지 않았다(필터가 안 맞음):\n{text}");
    assert!(!marker.exists(), "어댑터가 물려받은 GIT_CONFIG_PARAMETERS 의 filter 를 돌렸다");
}

/// `inherited_git_config_parameters_are_dropped` 가 자식 프로세스로 부르는 몸통. 부모가 환경을 주지 않으면 아무것도 하지 않는다
#[test]
#[ignore = "inherited_git_config_parameters_are_dropped 가 자식으로 부른다"]
fn inherited_env_child() {
    let (Some(repo), Some(marker)) = (std::env::var_os("CB_CHILD_REPO"), std::env::var_os("CB_CHILD_MARKER")) else {
        return;
    };
    assert!(std::env::var_os("GIT_CONFIG_PARAMETERS").is_some(), "부모가 GIT_CONFIG_PARAMETERS 를 물려주지 않았다");
    let (code, _, err) = guarded(Path::new(&repo), &["status", "--porcelain"]);
    assert_eq!(code, 0, "{err}");
    assert!(!Path::new(&marker).exists(), "어댑터가 물려받은 GIT_CONFIG_PARAMETERS 의 filter 를 돌렸다");
}
