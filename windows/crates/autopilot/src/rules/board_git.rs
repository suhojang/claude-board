//! 보드 합치기·승격 시트의 git 자리 박기·막기 규칙. 원본: `Sources/AutopilotRules/BoardGit.swift`.
//! 보드가 돌리는 git 은 샌드박스 밖이라, 실행 기록(run.json branches.repos.<저장소>)에 적힌 git 폴더가 지금과 다르면
//! 돌리지 않고(`board_git_changed`), 있으면 그 폴더로 GIT_DIR 을 박는다(`board_refs_pin`).
//! 순수 규칙이다 — rev-parse·inode(Windows 는 파일 ID) 읽기는 어댑터가 하고 이 파일은 값만 받는다.
//! inode 문자열은 Windows 에서 "볼륨 일련번호:파일 인덱스" 꼴로 어댑터가 넘긴다. 규칙은 문자열 비교만 한다.

/// 실행 기록의 저장소 하나 git 자리(run.json branches.repos.<저장소>). 보드가 읽어 값으로 넘긴다. 원본: `APGitRecord`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRecord {
    /// 공통 git 폴더(가지·객체) — 출발 때 적은 절대 경로
    pub common_dir: Option<String>,
    /// 그 자리의 "장치:inode"(바꿔치기 가리기)
    pub common_ino: Option<String>,
    /// 따로 작업 트리면 그 작업 트리의 git 폴더(.git/worktrees/<이름>)
    pub work_git_dir: Option<String>,
    pub work_git_ino: Option<String>,
}

const CHANGED_COMMON: &str = "git 폴더가 출발 때와 달라져";
const CHANGED_WORK: &str = "작업 트리 git 폴더가 출발 때와 달라져";

/// 저장소의 git 폴더가 출발 때와 같은가. 다르면 까닭(사람에게 보이는 글머리), 같거나 기록이 없으면(옛 실행) None.
/// inode 를 기록해 두었는데 지금 None 이면(못 읽음) 다르다고 본다(닫힌 쪽).
/// `work_tree_gone`: 따로 작업 트리 자리가 이제 없다(합친 뒤·버린 뒤 지움) — 그 git 폴더도 함께 지워졌으므로 공통 git 폴더만 본다.
/// 경로는 글자 그대로 견준다(대소문자·구분자 정규화 없음 — 기록과 지금을 같은 어댑터가 같은 꼴로 풀므로 닫힌 쪽이 안전하다).
/// 원본: `apBoardGitChanged`
pub fn board_git_changed(
    record: &GitRecord,
    live_common: Option<&str>,
    live_common_ino: Option<&str>,
    live_work_ino: Option<&str>,
    work_tree_gone: bool,
) -> Option<&'static str> {
    let c = record.common_dir.as_deref()?; // 기록 없음: 막지 않고 공통 보호(safe.bareRepository)로만
    if live_common != Some(c) {
        return Some(CHANGED_COMMON);
    }
    if let Some(ino) = record.common_ino.as_deref() {
        if live_common_ino != Some(ino) {
            return Some(CHANGED_COMMON);
        }
    }
    if !work_tree_gone && record.work_git_dir.is_some() {
        if let Some(wino) = record.work_git_ino.as_deref() {
            if live_work_ino != Some(wino) {
                return Some(CHANGED_WORK);
            }
        }
    }
    None
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `NSString.deletingLastPathComponent` 와 같은 꼴의 부모 경로. 끝의 구분자는 무시한다.
/// POSIX(`/w/repo` → `/w`, `/repo` → `/`, `repo` → 빈 글)에 더해 `\` 구분자·드라이브(`C:\w\repo` → `C:\w`, `C:\repo` → `C:\`)도 푼다.
pub(crate) fn parent_path(p: &str) -> String {
    let t = p.trim_end_matches(is_sep);
    if t.is_empty() {
        return if p.is_empty() { String::new() } else { p[..1].to_string() };
    }
    match t.rfind(is_sep) {
        None => {
            // "C:repo" 같은 드라이브 상대 꼴은 드라이브 글자만 부모로 본다. 그 밖은 빈 글
            let b = t.as_bytes();
            if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' { t[..2].to_string() } else { String::new() }
        }
        Some(0) => t[..1].to_string(),
        Some(i) => {
            let parent = t[..i].trim_end_matches(is_sep);
            if parent.is_empty() { t[..1].to_string() } else { parent.to_string() }
        }
    }
}

/// 부모 경로가 뿌리(더 올라갈 데가 없는 자리)인가 — `/`, 빈 글, 드라이브 뿌리(`C:`·`C:\`·`C:/`), UNC 서버(`\\서버`), 접두 `\\?\` 드라이브.
pub(crate) fn is_root(parent: &str) -> bool {
    if parent.is_empty() {
        return true;
    }
    let body = parent.strip_prefix(r"\\?\").unwrap_or(parent);
    let t = body.trim_end_matches(is_sep);
    if t.is_empty() {
        return true;
    }
    let b = t.as_bytes();
    if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    // `\\서버` (공유 이름 없음): 구분자 둘로 시작하고 그 뒤에 구분자가 없다
    let mut cs = body.chars();
    if let (Some(a), Some(b2)) = (cs.next(), cs.next()) {
        if is_sep(a) && is_sep(b2) && !body[2..].trim_end_matches(is_sep).contains(is_sep) {
            return true;
        }
    }
    false
}

/// 보드가 ref·객체를 읽을 때 박는 env. 기록의 공통 git 폴더로 GIT_DIR 을 박고, 저장소 뿌리 위로는 git 이 저장소를 찾지 않게
/// GIT_CEILING_DIRECTORIES(뿌리의 부모)를 넣는다. 기록이 없으면 빈 배열(공통 보호로만).
/// GIT_WORK_TREE 는 넣지 않는다. 부모가 뿌리(`/`·드라이브 뿌리)이거나 비면 GIT_CEILING 을 넣지 않는다. 원본: `apBoardRefsPin`
pub fn board_refs_pin(record: &GitRecord, repo_root: &str) -> Vec<String> {
    let Some(c) = record.common_dir.as_deref() else { return Vec::new() };
    let mut env = vec![format!("GIT_DIR={c}")];
    let parent = parent_path(repo_root);
    if !is_root(&parent) {
        env.push(format!("GIT_CEILING_DIRECTORIES={parent}"));
    }
    env
}
