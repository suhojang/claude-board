//! 출발 때 경로 확인 — 작업 폴더·쓰기 범위·읽기 금지를 정규 경로로 바꾸고, 쓰면 안 되는 자리를 거른다(macOS `badRoot` 의 Windows 판).
//!
//! 규칙: 경로에 `..` 를 쓰지 않는다 · 네트워크 경로(`\\서버\공유`)는 Claude Code 권한 규칙이 받지 못해 네이티브 환경에서 쓰지 않는다 ·
//! `fs::canonicalize` 로 정규화한 경로(`\\?\` 뗀 것)를 고정 설정에 싣는다 · 정규화한 경로가 권한 규칙 꼴(`permission_path`)로 바뀌는지 미리 본다 ·
//! 쓰기 범위·작업 폴더는 드라이브 뿌리·홈 폴더와 그 위·`~\.claude` 아래·시스템 폴더 아래·시작 프로그램 폴더·실행 폴더·엔진 전용 폴더와 겹치면 안 된다.

use std::path::{Component, Path, PathBuf};

use claude_board_core::envs::EnvKind;
use serde_json::Value;

use crate::adapters::native_env::permission_path;
use crate::adapters::winproc::strip_verbatim;
use crate::model::QueueFile;

/// 쓰기 범위·작업 폴더로 쓸 수 없는 자리들
pub struct Forbidden {
    pub home: PathBuf,
    pub run_dir: PathBuf,
    pub private_dir: PathBuf,
    pub system_dirs: Vec<PathBuf>,
    pub startup: PathBuf,
}

impl Forbidden {
    /// 이 프로세스의 환경에서. 시스템 폴더는 `SystemRoot`·`windir`·`ProgramFiles`·`ProgramFiles(x86)`·`ProgramW6432`·`ProgramData`
    pub fn from_env(run_dir: &Path, private_dir: &Path) -> Self {
        let home = claude_board_core::paths::home();
        let system_dirs = ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"]
            .iter()
            .filter_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
            .map(|p| canon_lenient(&p))
            .collect();
        let startup = canon_lenient(&crate::engine::run::guard::startup_dir(&home));
        Self { home: canon_lenient(&home), run_dir: canon_lenient(run_dir), private_dir: canon_lenient(private_dir), system_dirs, startup }
    }

    /// 쓸 수 없는 까닭(쉬운 말). 없으면 None. `p` 는 정규화한 경로
    pub fn why_bad(&self, p: &Path) -> Option<String> {
        if p.parent().is_none() {
            return Some("드라이브 뿌리".to_string());
        }
        if under(&self.home, p) {
            return Some("홈 폴더 전체나 그 위".to_string());
        }
        if under(p, &self.home.join(".claude")) {
            return Some("~\\.claude 아래".to_string());
        }
        if self.system_dirs.iter().any(|s| under(p, s)) {
            return Some("시스템 폴더 아래".to_string());
        }
        if under(p, &self.startup) || under(&self.startup, p) {
            return Some("시작 프로그램 폴더와 겹침".to_string());
        }
        if under(p, &self.run_dir) || under(&self.run_dir, p) {
            return Some("실행 폴더와 겹침".to_string());
        }
        if under(p, &self.private_dir) || under(&self.private_dir, p) {
            return Some("엔진 전용 폴더와 겹침".to_string());
        }
        None
    }
}

/// 있는 데까지는 정규화하고(짧은 이름 8.3 → 긴 이름, `\?\` 뗌) 없는 뒷부분은 그대로 붙인다 — 아직 없는 엔진 전용 폴더도 견줄 수 있게
fn canon_lenient(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&cur) {
            let mut out = strip_verbatim(&c);
            out.extend(rest.iter().rev());
            return out;
        }
        match (cur.file_name().map(std::ffi::OsString::from), cur.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) if !parent.as_os_str().is_empty() => {
                rest.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

fn lower(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase())
}

/// `p` 가 `base` 와 같거나 그 아래인가(Windows 는 대소문자를 가리지 않는다)
fn under(p: &Path, base: &Path) -> bool {
    lower(p).starts_with(lower(base))
}

fn is_unc(p: &Path) -> bool {
    let t = p.to_string_lossy();
    let verbatim_drive = t.strip_prefix(r"\\?\").is_some_and(|r| !r.starts_with("UNC\\"));
    (t.starts_with(r"\\") || t.starts_with("//")) && !verbatim_drive
}

/// 사용자가 쓴 경로 하나를 정규 경로로. `must_exist` 면 폴더로 존재해야 한다(없는 경로는 `read_deny` 처럼 쓴 대로 둔다)
fn resolve(kind: EnvKind, raw: &str, must_exist: bool) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(format!("경로에 `..` 가 들어 있습니다: {raw}"));
    }
    if !path.is_absolute() {
        return Err(format!("절대 경로가 아닙니다: {raw}"));
    }
    // WSL 은 `\\wsl.localhost\<배포판>\…` 를 쓰므로 네트워크 꼴을 막지 않고 권한 규칙 꼴 확인도 환경 어댑터가 한다
    let native = kind != EnvKind::Wsl;
    if native && is_unc(path) {
        return Err(unc_message(raw));
    }
    let canon = if must_exist || path.exists() {
        match (native, std::fs::canonicalize(path)) {
            (_, Ok(c)) => strip_verbatim(&c),
            (false, Err(_)) if is_unc(path) => path.to_path_buf(),
            (_, Err(e)) => return Err(format!("폴더를 확인하지 못했습니다({e}): {raw}")),
        }
    } else {
        path.to_path_buf()
    };
    if native {
        if is_unc(&canon) {
            return Err(unc_message(raw));
        }
        permission_path(&canon).map_err(|e| e.message)?;
    }
    if must_exist && !canon.is_dir() {
        return Err(format!("폴더가 아니거나 없습니다: {raw}"));
    }
    Ok(canon)
}

fn unc_message(raw: &str) -> String {
    format!("네트워크 경로(\\\\서버\\공유 꼴)는 쓸 수 없습니다: {raw} — 드라이브 글자가 붙은 경로로 쓰세요.")
}

/// 확인을 마친 경로들(고정 설정에 싣는다)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    /// 단위마다 작업 폴더(queue 차례)
    pub cwds: Vec<PathBuf>,
    pub write_roots: Vec<String>,
    pub read_deny: Vec<String>,
}

fn string_list(run: &Value, key: &str, problems: &mut Vec<String>) -> Vec<String> {
    match run.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(_) => {
            problems.push(format!("run.json 의 {key} 는 글자 배열이어야 합니다."));
            Vec::new()
        }
    }
}

fn dedup(list: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in list {
        if !out.iter().any(|q| lower(q) == lower(&p)) {
            out.push(p);
        }
    }
    out
}

/// 단위 작업 폴더·`write_roots`·`read_deny` 를 모두 확인한다. 문제는 모아서 한꺼번에 돌려준다
pub fn check(f: &Forbidden, kind: EnvKind, queue: &QueueFile, run: &Value) -> Result<Checked, Vec<String>> {
    let mut problems: Vec<String> = Vec::new();
    let mut checked = Checked::default();
    for n in 1..=queue.len() {
        let raw = queue[n - 1].cwd.get().cloned().unwrap_or_default();
        let title = queue[n - 1].title.get().cloned().unwrap_or_else(|| format!("단위 {n}"));
        match resolve(kind, &raw, true) {
            Ok(c) => match f.why_bad(&c) {
                Some(why) => problems.push(format!("단위 {n}({title}): 작업 폴더로 {raw} 를 쓸 수 없습니다({why}).")),
                None => checked.cwds.push(c),
            },
            Err(why) => problems.push(format!("단위 {n}({title}): 작업 폴더(cwd)를 쓸 수 없습니다 — {why}")),
        }
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for raw in string_list(run, "write_roots", &mut problems) {
        match resolve(kind, &raw, true) {
            Ok(c) => match f.why_bad(&c) {
                Some(why) => problems.push(format!("write_roots 에 {raw} 를 넣을 수 없습니다({why}).")),
                None => roots.push(c),
            },
            Err(why) => problems.push(format!("write_roots 를 쓸 수 없습니다 — {why}")),
        }
    }
    let mut denies: Vec<PathBuf> = Vec::new();
    for raw in string_list(run, "read_deny", &mut problems) {
        match resolve(kind, &raw, false) {
            Ok(c) => denies.push(c),
            Err(why) => problems.push(format!("read_deny 를 쓸 수 없습니다 — {why}")),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let text = |v: Vec<PathBuf>| v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    checked.write_roots = text(dedup(roots));
    checked.read_deny = text(dedup(denies));
    Ok(checked)
}
