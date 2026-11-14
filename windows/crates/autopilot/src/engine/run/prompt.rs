//! 모델에게 주는 글 — macOS `Sources/ClaudeAutopilot/main.swift` 의 시스템 프롬프트(602~613)·결과 기록 형식(615~632)·
//! 첫 입력(`firstMessage` 3327~3391)을 옮겼다. 두 상수는 글자 그대로이고, 첫 입력은 Windows MVP 몫(시나리오·수용 기준·Jira·근거 묶음·세트 줄 제외)만 낸다.
//! 순수 함수다 — 파일을 읽지 않는다.

use claude_board_core::paths;

/// 원본: `systemPromptText`(main.swift 603~612). 끝 줄바꿈 없음
pub const SYSTEM_PROMPT: &str = r#"이 세션은 Claude Board 자율주행 모드로 돈다.
- 사람에게 묻지 않는다. 사람 몫(되돌리기 어려운 바깥 반영, 보안과 책임, 운영 값과 용량, 자율주행 자신의 규칙·시험·종료 조건, 사람이 손댄 영역)에 걸리는 부분은 하지 않고 result.json 의 next_start 에 적는다. 걸리지 않으면 근거 문서와 대조한 권고안을 채택하고 decisions_applied 에 적는다.
- 허용 범위 밖의 도구는 묻지 않고 거부된다. 거부되면 다른 길로 하고, 길이 없으면 그 부분만 보류해 next_start 에 적는다.
- CLAUDE.md 와 .claude/ 아래 파일을 고치지 않는다. git push 와 Jira·Confluence 쓰기를 하지 않는다.
- 판정은 엔진이 판정 명령의 종료 코드로 한다. 판정 명령이나 시험 기준을 낮추거나 지우지 않는다.
- 사람이 읽는 글(답, Bash 도구의 description, result.json)은 모두 쉬운 한국어로 쓴다.
- 홈 폴더 전체를 뒤지는 검색(grep -r ~, find ~ 등)은 하지 않는다. 근거는 단위 글이 적은 경로와 작업 폴더에서 찾는다.
- 백그라운드로 띄운 명령은 턴을 끝내기 전에 끝낸다. 결과를 보낸 뒤 백그라운드 작업만 남아 오래 조용하면 엔진이 그 작업을 끝내고 넘어간다.
- 커밋은 엔진이 걸어 둔 지금 작업 가지에만 하고, 한 일은 턴을 끝내기 전에 그 가지에 커밋해 둔다. 다른 가지로 바꾸거나(switch·checkout·rebase) 다른 가지를 옮기거나 지우거나 맞추지(branch -f·update-ref·merge·reset) 않는다. 다른 가지 맞추기는 끝난 뒤 사람이 바뀐 줄을 보고 보드에서 하는 일이라서다(샌드박스도 그런 쓰기를 막는다). 프로젝트 규칙이 dev·live·master 같은 가지에 커밋·병합하거나 가지를 맞추라고 하면, 일은 지금 작업 가지에 커밋하고 맞추기는 하지 않은 채 result.json 의 next_start 에 무엇을 어느 가지로 맞추려 했는지 적는다.
- 일을 마치면 $CLAUDE_BOARD_UNIT_DIR/result.json 을 쓰고 턴을 끝낸다."#;

/// 원본: `resultFormat`(main.swift 616~631). 끝 줄바꿈 없음
pub const RESULT_FORMAT: &str = r#"{
  "unit": {"title": "쉬운 말 제목", "kind": "story|dev|fix|env", "ref": ""},
  "outcome": "done|failed|parked|capped",
  "headline": "한 문장 결론",
  "risks": ["알아 두실 위험(확인 요청이 아님)"],
  "summary": ["한 일 한 줄씩"],
  "tests": [{"name": "시험 이름(쉬운 말)", "result": "pass|fail|skip", "note": ""}],
  "changes": [{"what": "바꾼 것", "where": "저장소 경로"}],
  "decisions_applied": [{"what": "따른 결정", "basis": "근거(출발 카드, 근거 문서 절 등)"}],
  "sequence": [{"title": "흐름 그림 제목", "mermaid": "sequenceDiagram ..."}],
  "next_start": [{"title": "사람 몫이라 보류한 것", "why": "이유"}],
  "new_work": [{"title": "새로 찾은 일(다음 출발 대기열 후보)", "why": "이유"}],
  "new_tickets": [{"temp": "TMP-<단위 번호>-<차례>", "title": "실행 중에 새로 필요해진 티켓 제목(첫 입력이 임시 번호로 진행하라고 할 때만)", "why": "이유"}],
  "next": "다음에 할 일 한 문장",
  "refs": {"jira": "", "ssot": "", "commit": ""}
}"#;

/// 첫 입력에 넣는 단위 하나의 사실. 경로는 Windows 글 그대로다(WSL 이면 4.2b 가 환경 쪽 경로로 바꾼 글을 넘긴다)
pub struct PromptUnit<'a> {
    pub prompt: &'a str,
    pub title: &'a str,
    /// 작업 폴더
    pub cwd: &'a str,
    /// 판정 명령 글
    pub check: &'a str,
    /// 단위 폴더(`units\<n>`)
    pub unit_dir: &'a str,
    /// 단위 작업 폴더 밖에서 쓸 수 있는 자리
    pub write_roots: &'a [String],
    /// 걸어 둔 작업 가지. 없으면 가지 줄을 뺀다
    pub work_branch: Option<&'a str>,
    /// 이 실행에 샌드박스가 걸리는가
    pub sandbox: bool,
    /// 출발 때 고른 답 파일(`answers.json`) 경로. 카드 파일(`cards.json`)은 같은 폴더에 있다
    pub answers: Option<&'a str>,
}

/// 원본: `firstMessage`(main.swift 3327~3391)의 Windows MVP 판.
/// 차례: prompt → `\n\n---\n` → 머리 줄 → 작업 폴더 → 판정 명령 → 티켓 없음 줄 → 고른 답(있으면) → 쓰기 범위 →
/// 작업 가지(있으면) → 단위 폴더가 `~\.claude` 아래일 때의 Bash 안내 → result.json 안내 → 형식 코드 블록.
pub fn first_message(u: &PromptUnit) -> String {
    let mut s = format!("{}\n\n---\n", u.prompt);
    s += &format!("이 일은 Claude Board 자율주행의 작업 단위 하나입니다(「{}」).\n", u.title);
    s += &format!("- 작업 폴더: {}\n", u.cwd);
    s += &format!(
        "- 판정 명령: `{}` (일을 마치면 엔진이 작업 폴더에서 돌려 종료 코드 0 이면 통과로 봅니다. 마치기 전에 직접 돌려 보세요.)\n",
        u.check
    );
    s += "- 이 단위에 쓸 Jira 티켓 번호는 없습니다. 새 티켓이 있어야 하는 부분은 하지 말고 next_start 에 적으세요.\n";
    if let Some(answers) = u.answers {
        s += &format!(
            "- 출발 때 고른 답: {answers} (카드: {}). 따를 답이 있으면 따르고 decisions_applied 에 적으세요.\n",
            sibling(answers, "cards.json")
        );
    }
    s += &write_scope_line(u);
    if let Some(branch) = u.work_branch {
        s += &format!(
            "- 작업 가지: `{branch}`. 커밋은 이 가지에만 합니다. 가지를 바꾸거나 다른 가지를 옮기지 않습니다(다른 가지 맞추기는 끝난 뒤 사람이 보드에서 합니다).\n"
        );
    }
    if is_under(u.unit_dir, &paths::claude_dir().to_string_lossy()) {
        // ~/.claude 아래는 Claude Code 의 보호 경로라 Write·Edit 도구가 묻지 않는 모드에서 거부된다
        s += &format!(
            "- 단위 폴더(`{}`)는 Write·Edit 도구로 쓸 수 없습니다. result.json 과 작업 파일은 Bash 로 쓰세요(예: `cat > \"$CLAUDE_BOARD_UNIT_DIR/result.json\" <<'EOF'` … `EOF`).\n",
            u.unit_dir
        );
    }
    s += &format!(
        "- 일을 마치면 결과 기록을 `{}` 에 아래 형식의 JSON 하나로 쓰세요. 사람이 읽는 글은 모두 쉬운 한국어로 쓰고, 내부 번호(티켓·결정 번호 등)는 문장에 쓰지 말고 refs 에만 둡니다. 비용은 엔진이 채우므로 쓰지 않습니다. 해당 없는 칸은 빈 배열이나 빈 글자로 둡니다.\n\n",
        join_like(u.unit_dir, "result.json")
    );
    s += &format!("```json\n{RESULT_FORMAT}\n```\n");
    s
}

/// 쓰기 범위 줄. 샌드박스가 없으면 네트워크 제한 문장은 사실이 아니라 빼고, 권한 규칙만 막는다고 밝힌다
fn write_scope_line(u: &PromptUnit) -> String {
    let roots = if u.write_roots.is_empty() { String::new() } else { format!("와 {}", u.write_roots.join(", ")) };
    let head = format!("- 쓰기는 작업 폴더{roots}, 단위 폴더, 임시 폴더만 됩니다");
    if u.sandbox {
        format!("{head}(샌드박스). 네트워크는 허용한 곳만 나갑니다.\n")
    } else {
        format!("{head}(이 실행은 샌드박스 없이 돕니다. 권한 규칙으로만 막습니다).\n")
    }
}

/// `dir` 아래의 `name` — 구분자는 `dir` 이 쓴 것(역슬래시가 있으면 역슬래시, 아니면 `/`)
fn join_like(dir: &str, name: &str) -> String {
    let sep = if dir.contains('\\') { '\\' } else { '/' };
    format!("{}{sep}{name}", dir.trim_end_matches(['/', '\\']))
}

/// `path` 와 같은 폴더의 `name`(구분자는 path 가 쓴 것)
fn sibling(path: &str, name: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(i) => format!("{}{name}", &path[..=i]),
        None => name.to_string(),
    }
}

/// `path` 가 `root` 폴더 아래인가 — 구분자 `/`·`\` 와 대소문자를 가리지 않는다(Windows 경로)
pub fn is_under(path: &str, root: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").to_lowercase();
    let root = norm(root);
    norm(path).starts_with(&format!("{}/", root.trim_end_matches('/')))
}
