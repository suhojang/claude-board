//! 단위가 끝난 자리에 남은 변경을 어떻게 할지와 읽기 전용 자리에 남은 변경의 보관 규칙. 원본: `Sources/AutopilotRules/Leftovers.swift`.
//! git 은 부르지 않는다. 엔진이 읽은 작업 트리 상태 줄(`git status --porcelain`)과 출발 때 상태 줄을 값으로 받는다.
//! 상태 줄은 `git status --porcelain`(v1, -z 아님)의 줄 그대로다 — Windows git 도 경로 구분자를 `/` 로 내고,
//! 특수 글자·한글 경로는 따옴표로 싸서 낸다(`core.quotepath`). 그래서 이 규칙은 경로를 고치지 않고 그대로 돌려준다.

use std::collections::HashSet;

/// 가지를 바꾸기 전(또는 판정 전) 작업 트리에 남은 변경을 어떻게 하는가. 원본: `APLeftover`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leftover {
    /// 출발 때와 같다 — 그대로 바꾼다
    Clean,
    /// 이 실행이 만든 가지에 엔진이 커밋해 둔다(다음 작업 가지에 섞이지 않게)
    Commit,
    /// 읽기 전용으로 걸어 둔 스토리 가지: 엔진이 따로 보관(git stash)하고 이어 간다.
    /// `None` 이면 모두, 아니면 그 경로만(출발 때부터 있던 사람의 변경은 남긴다)
    Stash(Option<Vec<String>>),
    /// 바꾸지 않는다(그 단위를 건너뛴다)
    Refuse(LeftoverWhy),
}

/// 바꾸지 않는 까닭. 원본: `APLeftoverWhy`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftoverWhy {
    /// 읽기 전용 자리인데 출발 때부터 있던 변경과 갈라 보관할 수 없다(같은 파일·이름 바꿈·따옴표 경로·없어진 변경)
    ReadOnly,
    /// 출발 때부터 변경이 있었거나 이 실행이 만든 가지가 아니다
    Dirty,
}

/// `dirty`·`dirty_start` 는 `git status --porcelain` 의 줄, `on_story_branch` 는 지금 걸린 가지가 이 실행의 스토리 가지인가
/// (같은 스토리의 다른 저장소 작업 동안 읽기 전용으로 걸어 둔 자리), `made_by_run` 은 지금 걸린 가지를 이 실행이 만들었는가.
/// 스토리 가지에는 엔진도 커밋하지 않는다(스토리에는 판정을 통과한 작업만 합친다) — 대신 보관해 판정·다음 단위에 들어가지 않게 한다.
/// 원본: `apLeftover`
pub fn leftover(dirty: &[String], dirty_start: &[String], on_story_branch: bool, made_by_run: bool) -> Leftover {
    if dirty == dirty_start {
        return Leftover::Clean;
    }
    if on_story_branch {
        if dirty_start.is_empty() {
            return Leftover::Stash(None);
        }
        return match stash_paths(dirty, dirty_start) {
            Some(paths) => Leftover::Stash(Some(paths)),
            None => Leftover::Refuse(LeftoverWhy::ReadOnly),
        };
    }
    if !dirty_start.is_empty() || !made_by_run {
        return Leftover::Refuse(LeftoverWhy::Dirty);
    }
    Leftover::Commit
}

// 상태 줄 "XY 경로" 의 경로. 짧은 줄·이름 바꿈/복사(R·C)·따옴표 경로·빈 경로는 None.
// Swift 의 `line.count`(문자 단위)와 같게 글자(char) 로 센다. XY 두 글자 뒤 한 칸은 내용을 보지 않고 건너뛴다(원본과 같다)
fn status_path(line: &str) -> Option<&str> {
    if line.chars().count() <= 3 {
        return None;
    }
    let mut it = line.char_indices();
    let xy_end = it.nth(2).map(|(i, _)| i)?; // 세 번째 글자의 시작 = XY 두 글자의 끝
    let path_start = it.next().map_or(line.len(), |(i, _)| i); // 네 번째 글자의 시작
    let (xy, p) = (&line[..xy_end], &line[path_start..]);
    if xy.contains('R') || xy.contains('C') || p.starts_with('"') || p.is_empty() {
        return None;
    }
    Some(p)
}

/// 출발 때부터 변경이 있던 자리에서 새로 생긴 변경의 경로만 — 갈라 낼 수 없으면 `None`.
/// 이름 바꿈·복사(R·C, "옛 -> 새")와 따옴표로 싼 경로(특수 글자)는 pathspec 으로 정확히 고를 수 없어 갈라 내지 않는다.
/// 출발 때 있던 줄이 없어졌거나 새 줄이 출발 때 바뀐 파일과 같은 파일이면 보관하면 사람의 변경까지 가져가므로 갈라 내지 않는다.
/// 원본: `apStashPaths`
pub fn stash_paths(dirty: &[String], dirty_start: &[String]) -> Option<Vec<String>> {
    let have: HashSet<&str> = dirty.iter().map(String::as_str).collect();
    if !dirty_start.iter().all(|s| have.contains(s.as_str())) {
        return None;
    }
    let start_paths: Vec<&str> = dirty_start.iter().filter_map(|l| status_path(l)).collect();
    let mut out: Vec<String> = Vec::new();
    for line in dirty.iter().filter(|l| !dirty_start.contains(l)) {
        let p = status_path(line)?;
        // 같은 파일이거나 한쪽이 다른 쪽의 폴더(추적 안 하는 폴더 "?? dir/")면 갈라 낼 수 없다
        if start_paths
            .iter()
            .any(|s| *s == p || (s.ends_with('/') && p.starts_with(s)) || (p.ends_with('/') && s.starts_with(p)))
        {
            return None;
        }
        out.push(p.to_string());
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 보관(git stash) 메시지 — 실행 id·단위·저장소·가지. `git stash list` 에 "On <가지>: <메시지>" 로 보인다. 원본: `apStashMessage`
pub fn stash_message(run: &str, unit: Option<i64>, repo: &str, branch: &str) -> String {
    let unit = unit.map_or_else(|| "?".to_string(), |u| u.to_string());
    format!("자율주행 {run} 단위 {unit} · {repo}: 읽기 전용으로 걸어 둔 {branch} 에 남은 변경(엔진이 보관)")
}

/// 보관한 변경을 누구 몫으로 적을까 — 판정 전이면 그 단위, 단위를 시작하기 전·끝 정리 때는 그 앞에서 마지막으로 돈 단위.
/// `statuses` 는 대기열 차례대로의 단위 상태(엔진 queue.json status), `before` 는 이제 시작할 단위 번호(`None` 이면 끝 정리, 모든 단위를 본다).
/// 대기·건너뜀은 돌지 않았으므로 세지 않는다. 원본: `apStashUnit`
pub fn stash_unit(statuses: &[String], before: Option<i64>) -> Option<i64> {
    let count = i64::try_from(statuses.len()).unwrap_or(i64::MAX);
    let up_to = before.map_or(count, |b| b.saturating_sub(1)).min(count);
    if up_to <= 0 {
        return None;
    }
    (0..up_to as usize)
        .rev()
        .find(|&i| !matches!(statuses[i].as_str(), "pending" | "skipped"))
        .map(|i| i as i64 + 1)
}
