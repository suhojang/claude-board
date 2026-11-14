//! 이식 지문 시험 — macOS Swift 원본(규칙·시험)이 바뀌면 이식을 다시 보라고 실패한다.
//! "바뀌었다"만 잡고 "같게 옮겼다"는 보증하지 않는다(그것은 Rust 시험이 맡는다).
//! 판정: 항목마다 swift·tests 파일이 있고 LF 로 바꾼 sha256 이 같으며 rust 파일이 있어야 한다.
//! 구현된 `src/rules/*.rs`(첫 줄이 `//! (청크` 가 아닌 것)는 모두 PORTED.json 에 있어야 한다.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn sha_lf(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    Ok(Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect())
}

fn check_hash(root: &Path, rel: &str, want: &str, fails: &mut Vec<String>) {
    match sha_lf(&root.join(rel)) {
        Ok(got) if got == want => {}
        Ok(got) => fails.push(format!("{rel}: Swift 원본이 바뀌었다, 이식을 다시 보라 (새 sha256 {got})")),
        Err(e) => fails.push(format!("{rel}: 읽지 못함 ({e})")),
    }
}

#[test]
fn ported_files_match_swift_fingerprints() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = repo_root();
    let doc: Value = serde_json::from_slice(&fs::read(manifest.join("PORTED.json")).expect("PORTED.json")).expect("JSON");
    assert_eq!(doc["format"], "claude-board-autopilot-ported");
    assert_eq!(doc["version"], 1);

    let mut fails = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    for f in doc["files"].as_array().expect("files") {
        let swift = f["swift"].as_str().expect("swift");
        check_hash(&root, swift, f["sha256"].as_str().expect("sha256"), &mut fails);
        for t in f["tests"].as_array().into_iter().flatten() {
            check_hash(&root, t["swift"].as_str().expect("tests.swift"), t["sha256"].as_str().expect("tests.sha256"), &mut fails);
        }
        let rust = f["rust"].as_str().expect("rust");
        if !manifest.join(rust).is_file() {
            fails.push(format!("{rust}: 이식 파일이 없다"));
        }
        listed.push(rust.to_string());
    }
    for w in doc["windows_only"].as_array().into_iter().flatten() {
        listed.push(w.as_str().expect("windows_only").to_string());
    }

    for entry in fs::read_dir(manifest.join("src/rules")).expect("src/rules") {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let first = fs::read_to_string(&p).unwrap_or_default();
        if first.lines().next().unwrap_or("").starts_with("//! (청크") {
            continue;
        }
        let rel = format!("src/rules/{}", p.file_name().unwrap().to_string_lossy());
        if !listed.contains(&rel) {
            fails.push(format!("{rel}: 구현됐는데 PORTED.json 의 files·windows_only 에 없다"));
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}
