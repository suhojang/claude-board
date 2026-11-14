//! 따로 작업 트리에서 도는 실행(단일 저장소)의 출발 · 되돌리기 · 고정 설정 고치기를 실제 git 으로 돌리는 엔진 쪽.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 의 `startWorktree`·`rollbackBranches`(작업 트리 갈래)·`remapToWorktree`.
//! 판정은 `rules::prep_git`(출발 준비 확인)가 하고, 가지 기록은 `engine::branches` 의 `WorkRecord` 를 그대로 쓴다.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경을 거는 `adapters::git::ProcessGit`)로만 부른다. 저장소는 늘 호출 인자로 넘긴다.
//! - `start_worktree` 는 출발 가지 머리에서 작업 가지(`autopilot/<실행 id>`)를 파고 `root/<저장소 이름>` 아래에 작업 트리를 만든다.
//!   `root` 는 실행의 작업 트리 자리(원본 `home/.claude-board/worktrees/<실행 id>`, 호출자가 준다 — 시험에선 임시 폴더).
//!   원래 작업 트리·가지는 건드리지 않는다. 원본은 `"autopilot/" + id` 로 이름을 쓰지만 여기는 Windows 가지 이름 규칙(`branch_name`)을 쓴다.
//! - 고정 설정의 `worktree_files` 는 작업 트리로 옮긴다(복사 기본, `link` 는 읽기 전용). 원본은 심볼릭 링크를 만들지만
//!   Windows 에서는 만들지 않고 복사해 읽기 전용으로 둔다(짧은 경로 꼴을 쓰지 않는다 — Windows 포트 판정).
//! - 실수(가지 만들기 실패·설정 확인 막힘·옮기기 실패)면 되돌린다: 작업 트리 등록·폴더·빈 부모 폴더, 이어받은(`seed`) 가지가 아니면 작업 가지.
//!   옮기기·복사는 실패하면 조용히 넘기지 않고 오류로 터뜨린다(원본은 `try?` 로 덮었다 — 여기는 닫힌 쪽으로 엄격하다: 조용한 실패가 가장 큰 위험).

use crate::adapters::worktree::{add, add_existing, canonical_loose};
use crate::engine::branches::{branch_name, current_branch, WorkRecord};
use crate::engine::leftovers::status_lines;
use crate::engine::prep_git::{block_message, run_prep_git};
use crate::ports::Git;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 고정 설정 `worktree_files` 한 항목. 원본: `startWorktree` 가 읽는 `[String]` 또는 `[{path, link}]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeFile {
    /// 저장소 안의 상대 경로
    pub path: String,
    /// true 면 읽기 전용(원본은 바로 가기 — Windows 에선 복사해 읽기 전용으로 둔다)
    pub link: bool,
}

/// 출발이 돌려주는 것 (원본 `startWorktree` 의 되돌림 넷을 하나로 묶은 꼴)
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeStart {
    /// 만든 작업 트리 경로(한 가지 꼴 `C:/x/y`)
    pub path: String,
    /// run.json `branches.repos.<저장소>` 에 들어갈 가지 기록(`record_branch` 가 쓴다)
    pub record: WorkRecord,
    /// 작업 가지를 만들기 전의 가지 머리(가지 이름 → 커밋). run.json `branches.start.<저장소>` 에 들어간다
    pub start_heads: BTreeMap<String, String>,
    /// run.json `worktree` 칸에 들어갈 값(원본과 같은 꼴)
    pub worktree: Value,
}

/// 고정 설정의 `worktree_files` 를 항목들로 갈라 낸다. 문자열은 복사(`link: false`), 객체는 `path`·`link`.
/// 둘 다 아닌 항목은 원본과 같이 건너뛴다. 칸이 없으면 빈 목록
pub fn worktree_files_of(frozen: &Value) -> Vec<WorktreeFile> {
    let mut files = Vec::new();
    let Some(items) = frozen.get("worktree_files").and_then(Value::as_array) else {
        return files;
    };
    for item in items {
        if let Some(p) = item.as_str() {
            files.push(WorktreeFile { path: p.to_string(), link: false });
        } else if let Some(p) = item.get("path").and_then(Value::as_str) {
            files.push(WorktreeFile { path: p.to_string(), link: item.get("link").and_then(Value::as_bool).unwrap_or(false) });
        }
        // 둘 다 아니면 규칙이 건너뛴다(원본은 path 를 "" 로 만들어 마지막 복사 루프가 넘기게 한다)
    }
    files
}

/// 저장소 이름(마지막 성분). 원본 `repoName`(`NSString.lastPathComponent`)
fn repo_name(repo: &str) -> String {
    let stem = Path::new(repo).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| repo.to_string());
    // 성분 끝의 구분자(`…/`)는 원본과 같이 이름으로도 본다
    if stem.is_empty() { repo.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(repo).to_string() } else { stem }
}

fn io_fail(what: &str, e: std::io::Error) -> String {
    format!("{what}, 파일 시스템에 실패했습니다: {e}")
}

// ── git 질문 (원본 branchHeads·headOf) ─────────────────

/// 가지 머리 전체(가지 이름 → 커밋). 원본 `branchHeads`
fn branch_heads(git: &dyn Git, repo: &Path) -> Result<BTreeMap<String, String>, String> {
    let out = git
        .run(repo, &["for-each-ref", "--format=%(refname)%09%(objectname)", "refs/heads/"])
        .map_err(|e| format!("가지 머리를 읽지 못했습니다: {e}"))?;
    if out.code != 0 {
        return Err(format!("가지 머리를 읽지 못했습니다: {}", out.stderr.trim()));
    }
    let mut heads = BTreeMap::new();
    for line in out.stdout.lines() {
        let Some((name, sha)) = line.split_once('\t') else {
            return Err(format!("가지 머리 줄이 맞지 않습니다: {line:?}"));
        };
        heads.insert(name.strip_prefix("refs/heads/").unwrap_or(name).to_string(), sha.to_string());
    }
    Ok(heads)
}

/// 커밋 되짚기. 원본 `headOf` — git 이 못 찾으면 원본도 빈 값으로 두고 진행한다(가지 만들기가 그 아래에서 실패한다)
fn head_of(git: &dyn Git, repo: &Path, what: &str) -> Result<String, String> {
    let out = git
        .run(repo, &["rev-parse", "-q", "--verify", &format!("{what}^{{commit}}")])
        .map_err(|e| format!("커밋을 되짚지 못했습니다: {e}"))?;
    if out.code != 0 {
        return Ok(String::new());
    }
    Ok(out.stdout.trim().to_string())
}

/// 가지 기록 틀(원본 `repoRecord` 의 가지 칸). 상태는 `engine::leftovers` 의 상태 읽기(추적 안 하는 파일 포함, 무시 파일 제외)
fn record_for(
    git: &dyn Git,
    repo: &str,
    work: &str,
    base: &str,
    work_tree: &Path,
) -> Result<WorkRecord, String> {
    let repo_path = Path::new(repo);
    Ok(WorkRecord {
        repo: repo.to_string(),
        base_branch: base.to_string(),
        base_head: head_of(git, repo_path, &format!("refs/heads/{base}"))?,
        work: work.to_string(),
        original: current_branch(git, repo).map_err(|e| e.to_string())?,
        original_head: head_of(git, repo_path, "HEAD")?,
        dirty_start: status_lines(git, work_tree).map_err(|e| e.to_string())?,
    })
}

// ── 함께 둘 파일 (원본 startWorktree 의 worktree_files 루프) ──

/// 상대 경로 규칙. 잘못된 꼴이면 건너뛴다: 빈 경로·`..` 성분을 품은 경로·절대 경로(`/`·`\` 첫 글자, 드라이브 `C:`).
/// 원본은 `/` 성분으로만 `..` 를 보지만 Windows 에서는 `\` 성분을 같이 보고 드라이브 꼴도 절대 경로다
fn relative_is_safe(rel: &str) -> bool {
    // `:` 는 드라이브 글자(`C:`·`C:\x`·`C:x`)와 NTFS 대체 스트림이라 어디에 있든 막는다 —
    // `Path::join` 은 드라이브 접두사가 있는 인자가 앞 경로를 통째로 바꾸므로 작업 트리 밖을 지우고 쓰게 된다
    !(rel.is_empty()
        || rel.contains(':')
        || rel.split(['/', '\\']).any(|c| c == "..")
        || rel.starts_with('/')
        || rel.starts_with('\\')
        || !Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)))
}

/// 이미 있는 것(파일·폴더·바로 가기)을 지운다. 원본 `removeItem` — 둘 다 지운다
fn remove_any(dst: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(dst) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(dst),
        Ok(_) => std::fs::remove_file(dst),
    }
}

/// 읽기 전용으로 둔다(복사한 것 하나). Windows 는 읽기 전용 속성, 그 밖은 0444
fn make_read_only(p: &Path) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(p)?.permissions();
    #[cfg(windows)]
    perms.set_readonly(true);
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o444);
    }
    std::fs::set_permissions(p, perms)
}

/// 원본 자리를 작업 트리로 옮긴다(폴더면 그 아래 통째로). 바로 가기(`link`)는 Windows 에서 심볼릭 링크 대신
/// 복사해 읽기 전용으로 둔다 — Windows 포트 판정(모듈 앞머리의 약속)
fn place_worktree_file(repo: &Path, wt_cwd: &Path, file: &WorktreeFile) -> Result<(), String> {
    if !relative_is_safe(&file.path) {
        return Ok(()); // 원본 guard — 잘못된 상대 경로는 넘긴다
    }
    // 작업 트리의 .git 되짚기를 덮어쓰지 않는다(원본은 덮어 쓰고 작업 트리를 망가뜨린다 — 여기서는 잘못된 입력으로 터뜨린다)
    if file.path.split(['/', '\\']).next().is_some_and(|first| first.eq_ignore_ascii_case(".git")) {
        return Err(format!("worktree_files 의 {} 은(는) 작업 트리의 git 자리를 덮어 쓰므로 둘 수 없습니다", file.path));
    }
    let src = repo.join(&file.path);
    let src_meta = match std::fs::symlink_metadata(&src) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()), // 원본 guard — 원본이 없으면 넘긴다
        Err(e) => return Err(io_fail(&format!("{} 을 볼 수 없습니다", file.path), e)),
    };
    let dst = wt_cwd.join(&file.path);
    remove_any(&dst).map_err(|e| io_fail(&format!("{} 을 치우지 못했습니다", file.path), e))?;
    copy_tree(&src, &dst, &src_meta, file.link).map_err(|e| io_fail(&format!("{} 을 작업 트리로 옮기지 못했습니다", file.path), e))?;
    Ok(())
}

/// 원본(`m`: 이미 본 자리의 사정)을 목적지로 복사한다. `link` 이면 파일에 읽기 전용을 둔다
fn copy_tree(src: &Path, dst: &Path, m: &std::fs::Metadata, link: bool) -> std::io::Result<()> {
    if m.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let child = entry?;
            let child_meta = child.metadata()?;
            if child_meta.is_dir() {
                copy_tree(&child.path(), &dst.join(child.file_name()), &child_meta, link)?;
            } else {
                copy_leaf(&child.path(), &dst.join(child.file_name()), link)?;
            }
        }
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_leaf(src, dst, link)
}

/// 파일 하나 복사 + (`link` 이면) 읽기 전용
fn copy_leaf(src: &Path, dst: &Path, link: bool) -> std::io::Result<()> {
    std::fs::copy(src, dst)?;
    if link {
        make_read_only(dst)?;
    }
    Ok(())
}

// ── 출발 · 되돌리기 (원본 startWorktree·rollbackBranches) ──

/// 따로 작업 트리 모드의 출발을 만든다. 원본 `startWorktree` — 출발 가지에서 작업 가지를 파고,
/// `root/<저장소 이름>` 아래 작업 트리를 만들어 함께 둘 파일을 옮기고, git 설정의 include 를 다시 본다.
/// `root`: 실행의 작업 트리 자리(원본 `home/.claude-board/worktrees/<실행 id>`). `seeded`: 출발 전에 보드가
/// 이미 만든 가지를 이어받는지(⑪-2 — 이어받으면 가지를 만들지 않고 쓰고, 되돌릴 때도 지우지 않는다)
#[allow(clippy::too_many_arguments)] // 원복 `startWorktree` 와 계약 형태가 같다(1:1 형태 유지)
pub fn start_worktree(
    git: &dyn Git,
    repo: &str,
    id: &str,
    base: &str,
    seeded: bool,
    files: &[WorktreeFile],
    root: &Path,
    home: &str,
) -> Result<WorktreeStart, String> {
    let work = branch_name(id).map_err(|e| format!("실행 id 가 가지 이름으로 맞지 않습니다: {e}"))?;
    if repo.is_empty() {
        return Err("따로 작업 트리로 돌 저장소가 없습니다".to_string()); // 원본 guard
    }
    let repo_path = Path::new(repo);
    let wt_base = root.to_string_lossy().into_owned();
    let wt_cwd = PathBuf::from(format!("{wt_base}/{}", repo_name(repo))); // 원본: wtBase + "/" + repoName(r)
    let wt_path = wt_cwd.to_string_lossy().into_owned();
    if std::fs::symlink_metadata(&wt_cwd).is_ok() {
        return Err(format!("작업 트리 자리가 이미 있습니다: {wt_path}"));
    }
    // 보드 작업 트리 자리. 원본 mkdirPrivate — Windows 에선 그 폴더를 만들기만 한다(권한 좁히기는 보드 폴더 재산)
    let board = Path::new(home).join(".claude-board");
    let worktrees_spot = board.join("worktrees");
    for dir in [&board, &worktrees_spot, Path::new(&wt_base)] {
        std::fs::create_dir_all(dir).map_err(|e| io_fail(&format!("작업 트리 자리 {} 을 만들지 못했습니다", dir.display()), e))?;
    }
    let _ = git.run(repo_path, &["worktree", "prune"]); // 등록만 남은 옛 작업 트리를 치운다(실패해도 진행 — 원본과 같다)
    let mut start_heads: BTreeMap<String, String> = match branch_heads(git, repo_path) {
        Ok(h) => h, // 작업 가지를 만들기 전 머리
        Err(why) => {
            let _ = std::fs::remove_dir(&wt_base); // 방금 만든 빈 자리만 치운다(비어 있지 않으면 그대로 둔다)
            return Err(why);
        }
    };
    if seeded {
        start_heads.remove(&work); // 이어받은 가지는 "움직임"이 아니라 원래 있던 것
    }
    let added = if seeded {
        add_existing(git, repo_path, &wt_path, &work)
    } else {
        add(git, repo_path, &wt_path, &work, base)
    };
    if added.is_err() {
        // 원본은 wtBase 를 통째로 지우지만 `root` 는 호출자 몫이라 이 호출이 만든 작업 트리 자리만 지우고
        // 비어 있는 `root` 만 치운다(다른 것이 든 `root` 는 건드리지 않는다)
        let _ = remove_any(&wt_cwd);
        let _ = std::fs::remove_dir(&wt_base);
    }
    if let Err(e) = added {
        return Err(format!("{} 에 출발 가지 {} 에서 작업 트리를 만들지 못했습니다: {}", repo_name(repo), base, e));
    }
    for file in files {
        if let Err(why) = place_worktree_file(repo_path, &wt_cwd, file) {
            rollback_worktree(git, repo, &wt_path, &work, seeded);
            return Err(why);
        }
    }
    let record = match record_for(git, repo, &work, base, &wt_cwd) {
        Ok(r) => r,
        Err(why) => {
            rollback_worktree(git, repo, &wt_path, &work, seeded);
            return Err(why);
        }
    };
    // 작업 트리 git 폴더의 config.worktree(원래 것을 git 이 복사해 둔다)는 상대 경로의 기준 폴더가 달라
    // 출발 준비 확인과 다르게 풀릴 수 있어 다시 본다(원본 configIncludes). 막혔으면 되돌린다
    let prep = run_prep_git(git, &wt_path, &BTreeMap::new(), home);
    if prep.block.is_some() {
        rollback_worktree(git, repo, &wt_path, &work, seeded);
        let why = block_message(&prep, home)
            .unwrap_or_else(|| format!("작업 트리의 git 설정 확인이 막혔습니다({})", prep.block.unwrap_or_default()));
        return Err(format!("{why} — 작업 트리를 지웠습니다"));
    }
    let worktree_value = json!({
        "mode": "worktree",
        "repo": repo,
        "path": wt_path,
        "branch": work,
        "base": record.base_head,   // 원본: repoRecord 의 base.head
        "reason": "단일 저장소라 따로 작업 트리에서 돕니다(원래 작업 트리는 사람이 그대로 씁니다)"
    });
    Ok(WorktreeStart {
        path: wt_path,
        record,
        start_heads,
        worktree: worktree_value,
    })
}

/// 출발이 끝까지 가지 못했을 때 만든 작업 트리를 치운다(원본 `rollbackBranches` 의 작업 트리 갈래).
/// 모두 애쓰기(원본이 오류를 무시한 것과 같다) — 여기서 실패가 커지면 되돌리기가 실패라는 것만 문제지만
/// 지우는 일이 실패해도 남은 것은 사용자가 볼 수 있는 자리(git worktree list)로만 남는다.
/// 가지는 이어받은 것(`adopted` = 기록의 `seed`)이 아니라 만든 것이면 지운다
pub fn rollback_worktree(git: &dyn Git, repo: &str, wt: &str, work: &str, adopted: bool) {
    let repo_path = Path::new(repo);
    let _ = git.run(repo_path, &["worktree", "remove", "--force", wt]);
    let _ = git.run(repo_path, &["worktree", "prune"]);
    if !adopted {
        let _ = git.run(repo_path, &["branch", "-D", work]);
    }
    // 만든 자리의 부모 폴더가 비었으면 치운다(원본 contentsOfDirectory + removeItem)
    if let Some(base) = Path::new(wt).parent() {
        let empty = std::fs::read_dir(base).is_ok_and(|mut rd| rd.next().is_none());
        if empty {
            let _ = std::fs::remove_dir(base);
        }
    }
}

// ── 고정 설정 고치기 (원본 remapToWorktree) ──

/// 접두사 치환: `s` 가 원본 저장소 자신이거나 그 아래면 작업 트리 아래로 바꾼다.
/// 글 그대로(원본과 같다)와 Windows 꼴 차이(`\`·드라이브 글자 → `canonical_loose`)를 둘러 보고,
/// 바늘 뒤 구분자는 `/`·`\` 둘 다 받는다(원본은 `/` 만 — Windows 고정 설정이 `\` 를 섞어 두는 것을 받는다)
fn replace_prefix(s: &str, orig: &str, wt: &str) -> String {
    let canon = canonical_loose(orig);
    for needle in [orig, canon.as_str()] {
        if s == needle {
            return wt.to_string();
        }
        if matches!(s.as_bytes().get(needle.len()), Some(b'/') | Some(b'\\'))
            && s.get(..needle.len()) == Some(needle)
        {
            return format!("{wt}/{}", &s[needle.len() + 1..]);
        }
    }
    s.to_string()
}

/// 경계 치환: 원본 뒤에 `/`·공백·따옴표·줄바꿈·백틱이 오거나 글 끝일 때만 원본을 작업 트리로 바꾼다.
/// `toy` 와 `toy2` 가름(원본 boundary). 두 꼴(글 그대로·canonical_loose)을 한 바퀴에 본다
fn replace_boundary(text: &str, wt: &str, needles: &[String]) -> String {
    if needles.is_empty() {
        return text.to_string();
    }
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len() + wt.len());
    let mut i = 0; // 바이트 옮김 — 글 경계에서만 검기로 하므로 안전하다
    while i < b.len() {
        let mut matched = false;
        if text.is_char_boundary(i) {
            for needle in needles {
                let hits = b.get(i..).is_some_and(|rest| rest.starts_with(needle.as_bytes()));
                if !hits {
                    continue;
                }
                let j = i + needle.len();
                let after = b.get(j);
                if j == b.len() || matches!(after, Some(&b'/') | Some(&b' ') | Some(&b'"') | Some(&b'\'') | Some(&b'\n') | Some(&b'`')) {
                    out.push_str(wt);
                    i = j;
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }
        let step = text.get(i..).map(|rest| rest.chars().next().map_or(1, |c| c.len_utf8())).unwrap_or(1);
        out.push_str(&text[i..i + step]);
        i += step;
    }
    out
}

/// 치환 바늘 목록: 원본 그대로 + canonical_loose 꼴(같은 꼴이면 하나)
fn remap_needles(orig: &str) -> Vec<String> {
    let canon = canonical_loose(orig);
    if canon == orig {
        return vec![orig.to_string()];
    }
    vec![orig.to_string(), canon]
}

/// 단위 자리를 작업 트리 쪽으로 바꾼다(cmdStart 가 작업 트리를 만든 뒤).
/// cwd·write_roots 는 접두사 치환, check·precheck·action·prompt 는 경계 치환, context 는 그대로. 원본 `remapToWorktree`
pub fn remap_to_worktree(frozen: &mut Value, orig: &str, wt: &str) {
    if orig.is_empty() || wt.is_empty() {
        return; // 바늘 없이는 아무것도 바꾸지 않는다
    }
    let needles = remap_needles(orig);
    if let Some(roots) = frozen.get_mut("write_roots").and_then(Value::as_array_mut) {
        for root in roots.iter_mut() {
            if let Some(s) = root.as_str() {
                *root = Value::from(replace_prefix(s, orig, wt));
            }
        }
    }
    let Some(units) = frozen.get_mut("units").and_then(Value::as_array_mut) else {
        return;
    };
    for unit in units.iter_mut() {
        if let Some(slot) = unit.get_mut("cwd") {
            if let Some(s) = slot.as_str() {
                *slot = Value::from(replace_prefix(s, orig, wt));
            }
        }
        for key in ["check", "precheck", "action", "prompt"] {
            if let Some(slot) = unit.get_mut(key) {
                if let Some(s) = slot.as_str() {
                    *slot = Value::from(replace_boundary(s, wt, &needles));
                }
            }
        }
    }
    // context 는 그대로다(원본과 같다)
}
