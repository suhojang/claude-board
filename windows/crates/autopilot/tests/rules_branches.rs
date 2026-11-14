//! 가지 기록 읽기·합치기 상태 규칙(`rules::branches`) 시험. 원본: `Tests/AutopilotRulesTests/BranchesTests.swift`.
//! Swift 시험 6개(`MergeOpenTests`·`MergeMarkTests`·`StoryReposTests`·`DiscardRowsTests`·`CarriesBaseTests`·`FinishRootTests`)가 각각 `merge_open_표`·
//! `merge_mark_표`·`story_repos_표`·`discard_rows_표`·`carries_base_표`·`finish_root_표` 로 옮겨졌다.
//! 표의 줄은 Swift 에서 기계로 옮겼고 표 길이를 세어 줄이 빠지면 실패한다. 끝의 `extra_` 시험은 Swift 시험에 없는 것(직접 호출·경계·Windows 경로)이다.

use claude_board_autopilot::rules::branches::{
    carries_base, discard_rows, failed_rows, finish_root, merge_mark, merge_open, merge_sources, promoted_rows, step_done, story_repos, unresolved,
    MergeMark, Obj, Promoted, RefFate, Step,
};
use serde_json::{json, Value};

// 아래 표(TOYS·OPEN_ROWS·STORY_ROWS·mark_rows·discard_table·CARRIES_ROWS·FINISH_ROOT_ROWS)는 Swift 시험 파일에서 기계로 옮긴 것이다(손으로 다시 치지 않았다):
// #"…"# → r#"…"#, 나머지 글자는 그대로, 열거형 값(`.promoted("x")` 등)은 같은 뜻의 Rust 값으로. MarkRow 는 TOYS 를 같이 읽는다.

struct OpenRow {
    note: &'static str,
    branches: &'static str, // branches JSON, 또는 "toy:<실행 id>"
    steps: &'static str,
    with_autopilot: bool,
    open: bool,
}

struct StoryRow {
    note: &'static str,
    story: &'static str, // stories.<키> 의 JSON("nil" 은 값 없음), 또는 "toy:<실행 id>:<스토리 키>"
    repos: &'static str, // 결과 JSON
}

struct MarkRow {
    note: &'static str,
    branches: &'static str, // branches JSON, 또는 "toy:<실행 id>"
    mark: Option<MergeMark>,
    label: Option<&'static str>,
    with_autopilot: bool, // 보드가 넘기는 값 — Swift 기본값 true
}

struct DiscardRow {
    note: &'static str,
    fate: RefFate,
    is_source: bool,
    recorded: bool,                   // 그 가지에 버린 줄이 이미 있음(Swift 기본값 false)
    discarded: Option<&'static str>, // JSON, None 이면 적지 않음
    failed: Option<&'static str>,
}

struct CarriesRow {
    note: &'static str,
    source: &'static str,
    after: Option<&'static str>,
    base: Option<&'static str>,
    want: bool,
}

struct FinishRootRow {
    note: &'static str,
    flag: bool,
    run_dev: Option<bool>,
    dev: bool,
    refuse: bool,
}

const TOYS: &[(&str, &str)] = &[
    ("20261027-1909-zz-ap52-todo", r#"{"discarded":[{"repo":"/w/zz-ap52-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/todo":{}},"stories":{}}"#),
    ("20261027-1918-zz-ap52-jira", r#"{"discarded":[{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/jira":{}},"stories":{"SHOP-12":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]},"SHOP-20":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#),
    ("20261028-1043-zz-ap53-jira", r#"{"discarded":[{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"autopilot"}],"promoted":[{"at":1793152437.734,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793152443.526,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793152494.429,"into":"master","repo":"/w/zz-ap53-toys/jira","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/jira":{}},"stories":{"SHOP-12":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-12/parent/SHOP-12","head":"68142a1","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-20":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-20/parent/SHOP-20","head":"68142a1","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#),
    ("merge-check-1", r#"{"discarded":[{"repo":"/w/toy/repo","source":"autopilot"}],"promoted":[],"repos":{"/w/toy/repo":{}},"stories":{}}"#),
    ("zz-ap51-inplace", r#"{"repos":{"/w/zz-ap51-toys/shop":{}},"stories":{}}"#),
    ("zz-ap51-jira", r#"{"repos":{"/w/zz-ap51-toys/jira":{}},"stories":{"SHOP-12":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"fix/_SHOP-12/parent/SHOP-12","head":"cb03493","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap51-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기(일부러 미통과)」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"feature/_SHOP-16/parent/SHOP-16","head":"2123d05","merged":["feature/_SHOP-16/SHOP-17"],"repo":"/w/zz-ap51-toys/jira","unmerged":[]}}}"#),
    ("zz-ap51-judge", r#"{"repos":{"/w/zz-ap51-toys/shop":{}},"stories":{}}"#),
    ("zz-ap51-long", r#"{"repos":{"/w/zz-ap51-toys/blog":{}},"stories":{}}"#),
    ("zz-ap51-wt", r#"{"repos":{"/w/zz-ap51-toys/todo":{}},"stories":{}}"#),
    ("zz-ap52-p1", r#"{"discarded":[{"repo":"/w/zz-ap52-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/todo":{}},"stories":{}}"#),
    ("zz-ap52-pj", r#"{"discarded":[{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-12"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-16"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap52-toys/jira","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap52-toys/jira":{}},"stories":{"SHOP-12":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]},"SHOP-20":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"repo":"/w/zz-ap52-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#),
    ("zz-ap53-cpu", r#"{"discarded":[{"repo":"/w/zz-ap53-toys/notes","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/notes":{}},"stories":{}}"#),
    ("zz-ap53-inplace", r#"{"promoted":[{"at":1793151315.679,"into":"dev","repo":"/w/shop/shop-core","source":"autopilot"},{"at":1793151315.779,"into":"dev","repo":"/w/shop/shop-web","source":"autopilot"},{"at":1793151466.062,"into":"live","repo":"/w/shop/shop-core","source":"autopilot"},{"at":1793151466.171,"into":"live","repo":"/w/shop/shop-web","source":"autopilot"},{"at":1793151469.738,"into":"master","repo":"/w/shop/shop-core","source":"branch:live"},{"at":1793151469.847,"into":"master","repo":"/w/shop/shop-web","source":"branch:live"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{}}"#),
    ("zz-ap53-jira", r#"{"discarded":[{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-20"},{"repo":"/w/zz-ap53-toys/jira","source":"autopilot"}],"promoted":[{"at":1793151645.249,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-16"},{"at":1793151648.79,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-16"},{"at":1793151683.177,"into":"dev","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793151708.494,"into":"live","repo":"/w/zz-ap53-toys/jira","source":"story:SHOP-12"},{"at":1793151757.678,"into":"master","repo":"/w/zz-ap53-toys/jira","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/jira":{}},"stories":{"SHOP-12":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-12/parent/SHOP-12","head":"e9f07ed","merged":["fix/_SHOP-12/SHOP-13"],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]},"SHOP-16":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"feature/_SHOP-16/parent/SHOP-16","head":"d15100a","merged":["feature/_SHOP-16/SHOP-17","feature/_SHOP-16/SHOP-18"],"repo":"/w/zz-ap53-toys/jira","unmerged":[]},"SHOP-20":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-20/parent/SHOP-20","head":"3034f74","merged":[],"repo":"/w/zz-ap53-toys/jira","unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}}"#),
    ("zz-ap53-wt", r#"{"discarded":[{"repo":"/w/zz-ap53-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/todo":{}},"stories":{}}"#),
    ("zz-ap53-wt2", r#"{"promoted":[{"at":1793151235.151,"into":"dev","repo":"/w/zz-ap53-toys/notes","source":"autopilot"},{"at":1793151246.682,"into":"live","repo":"/w/zz-ap53-toys/notes","source":"autopilot"},{"at":1793151250.104,"into":"master","repo":"/w/zz-ap53-toys/notes","source":"branch:live"}],"repos":{"/w/zz-ap53-toys/notes":{}},"stories":{}}"#),
    ("zz-ap53-wt3", r#"{"discarded":[{"repo":"/w/zz-ap53-toys/todo","source":"autopilot"}],"promoted":[],"repos":{"/w/zz-ap53-toys/todo":{}},"stories":{}}"#),
    ("zz-cpu-ap", r#"{"discarded":[{"repo":"/w/cpu/toy2","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy2":{}},"stories":{}}"#),
    ("zz-cpu-ap2", r#"{"discarded":[{"repo":"/w/cpu/toy2","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy2":{}},"stories":{}}"#),
    ("zz-cpu-run", r#"{"discarded":[{"repo":"/w/cpu/toy","source":"autopilot"}],"promoted":[],"repos":{"/w/cpu/toy":{}},"stories":{}}"#),
    ("zz-merge-gui", r#"{"discarded":[{"repo":"/w/toy/repo2","source":"autopilot"}],"promoted":[],"repos":{"/w/toy/repo2":{}},"stories":{}}"#),
    ("zz-mr-before", r#"{"discarded":[{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"}],"promoted":[],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-1":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"3afdae8","merged":[],"repo":"/w/shop/shop-web","unmerged":[{"task":"features/SHOP-3","why":"미통과"},{"task":"features/SHOP-9","why":"미통과"}]},"SHOP-4":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"879eaea","merged":["features/SHOP-5"],"repo":"/w/shop/shop-web","unmerged":[]},"SHOP-6":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"repo":"/w/shop/shop-core","unmerged":[{"task":"features/SHOP-7","why":"미통과"},{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}}"#),
    ("zz-mr-jira", r#"{"discarded":[{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"story:SHOP-6"},{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"},{"repo":"/w/shop/shop-web","source":"story:SHOP-4"}],"failed":[{"at":1793155257.15,"into":"live","op":"merge","repo":"/w/shop/shop-web","source":"story:SHOP-1"}],"promoted":[{"at":1793155244.736,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155245.016,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-1"},{"at":1793155256.808,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155260.138,"into":"master","repo":"/w/shop/shop-core","source":"branch:live"},{"at":1793155260.138,"into":"master","repo":"/w/shop/shop-web","source":"branch:live"},{"at":1793155263.43,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-1"},{"at":1793155263.672,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-1"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-1":{"repos":{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-1","head":"9b14bde","merged":["features/SHOP-2","features/SHOP-9"],"unmerged":[]},"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"4ebe25d","merged":["features/SHOP-3"],"unmerged":[]}}},"SHOP-4":{"repos":{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"1a34fd1","merged":["features/SHOP-5"],"unmerged":[]}}},"SHOP-6":{"repos":{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"}]},"/w/shop/shop-web":{"branch":"features/parent/SHOP-6","merged":[],"unmerged":[{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}}}}"#),
    ("zz-mr-jira2", r#"{"discarded":[{"repo":"/w/shop/shop-core","source":"story:SHOP-21"},{"repo":"/w/shop/shop-web","source":"story:SHOP-21"},{"repo":"/w/shop/shop-core","source":"story:SHOP-21"},{"repo":"/w/shop/shop-web","source":"story:SHOP-21"},{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"story:SHOP-14"},{"repo":"/w/shop/shop-web","source":"story:SHOP-14"}],"failed":[{"at":1793156028.982,"into":"live","op":"merge","repo":"/w/shop/shop-web","source":"story:SHOP-11"}],"promoted":[{"at":1793156001.433,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156001.545,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-11"},{"at":1793156028.713,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156061.999,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-11"},{"at":1793156062.09,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-11"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-11":{"repos":{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-11","head":"5b7764b","merged":["features/SHOP-12"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-11","head":"8fad31a","merged":["features/SHOP-13"],"unmerged":[]}}},"SHOP-14":{"repos":{"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-14","head":"f6f9134","merged":["features/SHOP-15"],"unmerged":[]}}},"SHOP-21":{"repos":{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-21","head":"85d9f53","merged":["features/SHOP-22"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-21","head":"378b7b1","merged":["features/SHOP-23"],"unmerged":[]}}}}}"#),
    ("zz-mr-jira3", r#"{"discarded":[{"repo":"/w/shop/shop-core","source":"autopilot"},{"repo":"/w/shop/shop-web","source":"autopilot"}],"promoted":[{"at":1793156997.223,"into":"dev","repo":"/w/shop/shop-core","source":"story:SHOP-41"},{"at":1793156997.496,"into":"dev","repo":"/w/shop/shop-web","source":"story:SHOP-41"},{"at":1793157001.213,"into":"live","repo":"/w/shop/shop-core","source":"story:SHOP-41"},{"at":1793157001.467,"into":"live","repo":"/w/shop/shop-web","source":"story:SHOP-41"}],"repos":{"/w/shop/shop-core":{},"/w/shop/shop-web":{}},"stories":{"SHOP-41":{"repos":{"/w/shop/shop-core":{"base":"5b7764ba22717d93f035be308bb17e0ffa1d935b","branch":"features/parent/SHOP-41","head":"c9f4b55","merged":["features/SHOP-42"],"unmerged":[]},"/w/shop/shop-web":{"base":"8fad31a3a517151a6bb69037cc95afff17c28990","branch":"features/parent/SHOP-41","head":"a672aac","merged":["features/SHOP-43"],"unmerged":[]}}}}}"#),
    ("zz-mr-old", r#"{"discarded":[{"repo":"/w/zz-mr-toys/solo","source":"autopilot"}],"promoted":[{"at":1793156173.444,"into":"dev","repo":"/w/zz-mr-toys/solo","source":"story:SHOP-31"},{"at":1793156181.645,"into":"live","repo":"/w/zz-mr-toys/solo","source":"story:SHOP-31"}],"repos":{"/w/zz-mr-toys/solo":{}},"stories":{"SHOP-31":{"base":"695135148dec74c4cd5647391f6e5225380f3924","branch":"features/parent/SHOP-31","head":"c1ceaa4","merged":["features/SHOP-32","features/SHOP-33"],"repo":"/w/zz-mr-toys/solo","unmerged":[]}}}"#),
];

const OPEN_ROWS: &[OpenRow] = &[
    OpenRow { note: "가지 기록이 비었음 → 자율주행 가지가 남음", branches: r#"{}"#, steps: "", with_autopilot: true, open: true },
    OpenRow { note: "스토리가 없으면 withAutopilot 이 거짓이어도 자율주행 가지를 넣는다", branches: r#"{}"#, steps: "", with_autopilot: false, open: true },
    OpenRow { note: "자율주행 가지를 버림 → 남은 것 없음", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "", with_autopilot: true, open: false },
    OpenRow { note: "승격 차례가 없으면 어디든 합친 줄이 있으면 닿음", branches: r#"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "", with_autopilot: true, open: false },
    OpenRow { note: "마지막 자율주행 단계(dev)에 합침 → 남은 것 없음", branches: r#"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev", with_autopilot: true, open: false },
    OpenRow { note: "dev 에만 합쳤는데 마지막 자율주행 단계는 live → 남음", branches: r#"{"promoted":[{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev autopilot>live@dev", with_autopilot: true, open: true },
    OpenRow { note: "live 에 합쳤는데 기존 가지끼리의 마지막 단계(live → master)를 안 함 → 남음", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: true },
    OpenRow { note: "live 와 master 까지 → 남은 것 없음", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":20,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "버려서 닿은 합칠 쪽이 없으면 master 단계는 보지 않는다", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "한 저장소만 합치고 다른 저장소는 못 함(failed) → 단계가 끝나지 않음", branches: r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: true },
    OpenRow { note: "못 한 뒤 같은 저장소를 다시 합침(at 이 뒤) → 끝남", branches: r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":12,"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: false },
    OpenRow { note: "다시 합친 시각이 못 한 시각과 같아도 끝남(>=)", branches: r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":11,"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: false },
    OpenRow { note: "못 하기 전에 합친 줄은 다시 합친 것으로 세지 않는다", branches: r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":5,"into":"live","repo":"/r/b","source":"autopilot"},{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: true },
    OpenRow { note: "merge·promote 가 아닌 못 한 일(cleanup)은 단계를 막지 않는다", branches: r#"{"failed":[{"at":11,"into":"live","op":"cleanup","repo":"/r/b","source":"autopilot"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: false },
    OpenRow { note: "시각이 없으면 0 으로 보고 견준다(다시 합친 것으로 셈)", branches: r#"{"failed":[{"into":"live","op":"merge","repo":"/r/b","source":"autopilot"}],"promoted":[{"into":"live","repo":"/r/a","source":"autopilot"},{"into":"live","repo":"/r/b","source":"autopilot"}]}"#, steps: "autopilot>live", with_autopilot: true, open: false },
    OpenRow { note: "master 올리기를 한 저장소만 못 함 → 남음", branches: r#"{"failed":[{"at":21,"into":"master","op":"promote","repo":"/r/b","source":"branch:live"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"},{"at":20,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "autopilot>live live>master@live", with_autopilot: true, open: true },
    OpenRow { note: "Jira: 한 스토리는 버리고 한 스토리는 live 까지 → 남은 것 없음(스토리 기록에 head — 가지를 만든 스토리)", branches: r#"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: false },
    OpenRow { note: "Jira: 스토리 하나가 아직 → 남음", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: true },
    OpenRow { note: "스토리는 합쳤지만 withAutopilot 이라 자율주행 가지가 남음", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S1"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}}}}"#, steps: "autopilot>live", with_autopilot: true, open: true },
    OpenRow { note: "5-3b ④: 어느 저장소에도 스토리 가지를 만들지 않은 스토리(head 없음)는 합칠 쪽이 아니다 → S2 만 live 에 합쳐 남은 것 없음(전에는 S1 이 늘 남음)", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","merged":[],"unmerged":[{"task":"f/T1","why":"건너뜀"}]},"/r/b":{"branch":"f/S1","merged":[]}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: false },
    OpenRow { note: "5-3b ④: 옛 꼴(repo 하나)도 head 가 없으면 합칠 쪽이 아니다", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{"branch":"f/S1","repo":"/r/a","unmerged":[]},"S2":{"branch":"f/S2","head":"h2","repo":"/r/a"}}}"#, steps: "autopilot>live", with_autopilot: false, open: false },
    OpenRow { note: "5-3b ④: 한 저장소에라도 head 가 있으면 합칠 쪽이다", branches: r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"story:S2"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1"},"/r/b":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: true },
    OpenRow { note: "5-3b ④: 모든 스토리가 가지를 만들지 않았으면 자율주행 가지만 합칠 쪽(스토리가 없을 때와 같다) → 남음", branches: r#"{"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: true },
    OpenRow { note: "5-3b ④: 그 자율주행 가지를 버리면 → 남은 것 없음", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}],"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1"}}}}}"#, steps: "autopilot>live", with_autopilot: false, open: false },
    OpenRow { note: "5-3b ④: 보드 밖에서 지운 가지를 엔진 discard 가 버린 것으로 적음(left missing) → 남은 것 없음", branches: r#"{"discarded":[{"branch":"autopilot/x","left":"missing","repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev autopilot>live@dev", with_autopilot: true, open: false },
    OpenRow { note: "stories 가 빈 객체면 자율주행 가지를 넣는다", branches: r#"{"stories":{}}"#, steps: "", with_autopilot: false, open: true },
    OpenRow { note: "stories 가 객체가 아니면 스토리 없음으로 본다", branches: r#"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"}],"stories":["S1"]}"#, steps: "", with_autopilot: false, open: false },
    OpenRow { note: "자율주행 단계 없는 차례(live → master 뿐): 합친 줄은 닿음, master 를 안 함 → 남음", branches: r#"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"}]}"#, steps: "live>master", with_autopilot: true, open: true },
    OpenRow { note: "자율주행 단계 없는 차례에서 master 까지 함 → 남은 것 없음", branches: r#"{"promoted":[{"at":1,"into":"dev","repo":"/r/a","source":"autopilot"},{"at":2,"into":"master","repo":"/r/a","source":"branch:live"}]}"#, steps: "live>master", with_autopilot: true, open: false },
    OpenRow { note: "source 없는 버린 줄은 아무것도 버리지 않는다", branches: r#"{"discarded":[{"repo":"/r/a"}]}"#, steps: "", with_autopilot: true, open: true },
    OpenRow { note: "discarded 가 배열이 아니면 버린 것 없음", branches: r#"{"discarded":{"source":"autopilot"}}"#, steps: "", with_autopilot: true, open: true },
    OpenRow { note: "into 없는 합친 줄은 빈 가지로 읽혀 dev 단계에 닿지 않음", branches: r#"{"promoted":[{"repo":"/r/a","source":"autopilot"}]}"#, steps: "autopilot>dev", with_autopilot: true, open: true },
    OpenRow { note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:20261027-1909-zz-ap52-todo", steps: "autopilot>dev autopilot>live@dev live>main@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:20261027-1918-zz-ap52-jira", steps: "autopilot>dev autopilot>live@dev live>main@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:20261028-1043-zz-ap53-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap51-inplace(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-inplace", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 보드가 넘기는 값: 차례 0단계 · 자율주행 가지 뺌", branches: "toy:zz-ap51-jira", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap51-judge(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-judge", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-long(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-long", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-wt(저장소 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-ap51-wt", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap52-p1", steps: "autopilot>dev autopilot>live@dev live>main@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-ap52-pj", steps: "autopilot>dev autopilot>live@dev live>main@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-cpu", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-inplace", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-ap53-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt2", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 3단계", branches: "toy:zz-ap53-wt3", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-ap", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-ap2", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 보드가 넘기는 값: 차례 0단계", branches: "toy:zz-cpu-run", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-before", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira2", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira3", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 보드가 넘기는 값: 차례 3단계 · 자율주행 가지 뺌", branches: "toy:zz-mr-old", steps: "autopilot>dev autopilot>live@dev live>master@live", with_autopilot: false, open: true },
    OpenRow { note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261027-1909-zz-ap52-todo", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261027-1909-zz-ap52-todo", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261027-1918-zz-ap52-jira", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261027-1918-zz-ap52-jira", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:20261028-1043-zz-ap53-jira", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:20261028-1043-zz-ap53-jira", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 merge-check-1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:merge-check-1", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 merge-check-1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:merge-check-1", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap51-inplace(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-inplace", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-inplace(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-inplace", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-jira", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-jira", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap51-judge(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-judge", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-judge(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-judge", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap51-long(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-long", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-long(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-long", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap51-wt(저장소 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap51-wt", steps: "", with_autopilot: true, open: true },
    OpenRow { note: "장난감 zz-ap51-wt(저장소 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap51-wt", steps: "", with_autopilot: false, open: true },
    OpenRow { note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap52-p1", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap52-p1", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap52-pj", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap52-pj", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-cpu", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-cpu", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-inplace", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-inplace", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-jira", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-jira", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt2", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt2", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-ap53-wt3", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-ap53-wt3", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-ap", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-ap", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-ap2", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-ap2", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-cpu-run", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-cpu-run(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-cpu-run", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-merge-gui(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-merge-gui", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-merge-gui(저장소 1·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-merge-gui", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-before", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-before", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira2", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira2", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-jira3", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-jira3", steps: "", with_autopilot: false, open: false },
    OpenRow { note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 차례 없음 · 자율주행 가지 넣음", branches: "toy:zz-mr-old", steps: "", with_autopilot: true, open: false },
    OpenRow { note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 차례 없음 · 자율주행 가지 뺌", branches: "toy:zz-mr-old", steps: "", with_autopilot: false, open: false },
];

const STORY_ROWS: &[StoryRow] = &[
    StoryRow { note: "값 없음 → 빈 결과", story: r#"nil"#, repos: r#"{}"# },
    StoryRow { note: "객체가 아님 → 빈 결과", story: r#""x""#, repos: r#"{}"# },
    StoryRow { note: "빈 객체 → 빈 결과", story: r#"{}"#, repos: r#"{}"# },
    StoryRow { note: "옛 꼴인데 repo 가 빈 글자 → 빈 결과", story: r#"{"branch":"f/S1","repo":""}"#, repos: r#"{}"# },
    StoryRow { note: "옛 꼴(스토리 하나에 저장소 하나)은 repo 를 떼고 그 저장소 하나로", story: r#"{"base":"live","branch":"f/S1","head":"h1","merged":["T1"],"repo":"/r/a","unmerged":[]}"#, repos: r#"{"/r/a":{"base":"live","branch":"f/S1","head":"h1","merged":["T1"],"unmerged":[]}}"# },
    StoryRow { note: "새 꼴(repos)은 저장소마다 그대로", story: r#"{"repos":{"/r/a":{"branch":"f/S1","head":"h1"},"/r/b":{"branch":"f/S1"}}}"#, repos: r#"{"/r/a":{"branch":"f/S1","head":"h1"},"/r/b":{"branch":"f/S1"}}"# },
    StoryRow { note: "새 꼴에서 객체가 아닌 저장소 칸은 뺀다", story: r#"{"repos":{"/r/a":{"branch":"f/S1"},"/r/b":"x"}}"#, repos: r#"{"/r/a":{"branch":"f/S1"}}"# },
    StoryRow { note: "repos 가 빈 객체면 repo 가 있어도 빈 결과(새 꼴이 먼저)", story: r#"{"repo":"/r/a","repos":{}}"#, repos: r#"{}"# },
    StoryRow { note: "repos 가 객체가 아니면 옛 꼴로 읽어 repos 칸까지 그대로 남긴다", story: r#"{"branch":"f/S1","repo":"/r/a","repos":"x"}"#, repos: r#"{"/r/a":{"branch":"f/S1","repos":"x"}}"# },
    StoryRow { note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-12(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-12", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-16(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-16", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 20261027-1918-zz-ap52-jira 스토리 SHOP-20(옛 꼴)", story: "toy:20261027-1918-zz-ap52-jira:SHOP-20", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 20261028-1043-zz-ap53-jira 스토리 SHOP-12(옛 꼴)", story: "toy:20261028-1043-zz-ap53-jira:SHOP-12", repos: r#"{"/w/zz-ap53-toys/jira":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-12/parent/SHOP-12","head":"68142a1","merged":[],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 20261028-1043-zz-ap53-jira 스토리 SHOP-20(옛 꼴)", story: "toy:20261028-1043-zz-ap53-jira:SHOP-20", repos: r#"{"/w/zz-ap53-toys/jira":{"base":"68142a1e79b9f2c27e6e6d6998b024e36ec59534","branch":"fix/_SHOP-20/parent/SHOP-20","head":"68142a1","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 zz-ap51-jira 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap51-jira:SHOP-12", repos: r#"{"/w/zz-ap51-toys/jira":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"fix/_SHOP-12/parent/SHOP-12","head":"cb03493","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기(일부러 미통과)」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 zz-ap51-jira 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap51-jira:SHOP-16", repos: r#"{"/w/zz-ap51-toys/jira":{"base":"93989f7ae216008b7b9d908f1dfd2f298763fd34","branch":"feature/_SHOP-16/parent/SHOP-16","head":"2123d05","merged":["feature/_SHOP-16/SHOP-17"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-ap52-pj 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-12", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-12/parent/SHOP-12","head":"07fcf0c","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 zz-ap52-pj 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-16", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"feature/_SHOP-16/parent/SHOP-16","head":"07fcf0c","merged":[],"unmerged":[{"task":"feature/_SHOP-16/SHOP-17","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 zz-ap52-pj 스토리 SHOP-20(옛 꼴)", story: "toy:zz-ap52-pj:SHOP-20", repos: r#"{"/w/zz-ap52-toys/jira":{"base":"07fcf0c7077fa0980507364dc636b1274e50fe9f","branch":"fix/_SHOP-20/parent/SHOP-20","head":"07fcf0c","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 zz-ap53-jira 스토리 SHOP-12(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-12", repos: r#"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-12/parent/SHOP-12","head":"e9f07ed","merged":["fix/_SHOP-12/SHOP-13"],"unmerged":[{"task":"fix/_SHOP-12/SHOP-14","why":"미통과"},{"task":"fix/_SHOP-12/SHOP-15","why":"건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 zz-ap53-jira 스토리 SHOP-16(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-16", repos: r#"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"feature/_SHOP-16/parent/SHOP-16","head":"d15100a","merged":["feature/_SHOP-16/SHOP-17","feature/_SHOP-16/SHOP-18"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-ap53-jira 스토리 SHOP-20(옛 꼴)", story: "toy:zz-ap53-jira:SHOP-20", repos: r#"{"/w/zz-ap53-toys/jira":{"base":"3034f743946807ea94d71e86374fb3e59f40137e","branch":"fix/_SHOP-20/parent/SHOP-20","head":"3034f74","merged":[],"unmerged":[{"task":"fix/_SHOP-20/SHOP-21","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 zz-mr-before 스토리 SHOP-1(옛 꼴)", story: "toy:zz-mr-before:SHOP-1", repos: r#"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"3afdae8","merged":[],"unmerged":[{"task":"features/SHOP-3","why":"미통과"},{"task":"features/SHOP-9","why":"미통과"}]}}"# },
    StoryRow { note: "장난감 zz-mr-before 스토리 SHOP-4(옛 꼴)", story: "toy:zz-mr-before:SHOP-4", repos: r#"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"879eaea","merged":["features/SHOP-5"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-before 스토리 SHOP-6(옛 꼴)", story: "toy:zz-mr-before:SHOP-6", repos: r#"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"},{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 zz-mr-jira 스토리 SHOP-1(새 꼴)", story: "toy:zz-mr-jira:SHOP-1", repos: r#"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-1","head":"9b14bde","merged":["features/SHOP-2","features/SHOP-9"],"unmerged":[]},"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-1","head":"4ebe25d","merged":["features/SHOP-3"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-jira 스토리 SHOP-4(새 꼴)", story: "toy:zz-mr-jira:SHOP-4", repos: r#"{"/w/shop/shop-web":{"base":"3afdae8da6fac8170625164b9fe01753fb7ce793","branch":"features/parent/SHOP-4","head":"1a34fd1","merged":["features/SHOP-5"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-jira 스토리 SHOP-6(새 꼴)", story: "toy:zz-mr-jira:SHOP-6", repos: r#"{"/w/shop/shop-core":{"base":"f930d27547ac63fd875497a1e487dd43b291333f","branch":"features/parent/SHOP-6","head":"f930d27","merged":[],"unmerged":[{"task":"features/SHOP-7","why":"미통과"}]},"/w/shop/shop-web":{"branch":"features/parent/SHOP-6","merged":[],"unmerged":[{"task":"features/SHOP-8","why":"건너뜀: 기대는 앞 단위 「만료일 검사(코어)」 가 통과하지 못해 시작하지 않습니다."}]}}"# },
    StoryRow { note: "장난감 zz-mr-jira2 스토리 SHOP-11(새 꼴)", story: "toy:zz-mr-jira2:SHOP-11", repos: r#"{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-11","head":"5b7764b","merged":["features/SHOP-12"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-11","head":"8fad31a","merged":["features/SHOP-13"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-jira2 스토리 SHOP-14(새 꼴)", story: "toy:zz-mr-jira2:SHOP-14", repos: r#"{"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-14","head":"f6f9134","merged":["features/SHOP-15"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-jira2 스토리 SHOP-21(새 꼴)", story: "toy:zz-mr-jira2:SHOP-21", repos: r#"{"/w/shop/shop-core":{"base":"9b14bde17318c99530a442a8c5a2f38164cbba8d","branch":"features/parent/SHOP-21","head":"85d9f53","merged":["features/SHOP-22"],"unmerged":[]},"/w/shop/shop-web":{"base":"4ebe25df0ca7477f3f10bdd504765700cc9c15db","branch":"features/parent/SHOP-21","head":"378b7b1","merged":["features/SHOP-23"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-jira3 스토리 SHOP-41(새 꼴)", story: "toy:zz-mr-jira3:SHOP-41", repos: r#"{"/w/shop/shop-core":{"base":"5b7764ba22717d93f035be308bb17e0ffa1d935b","branch":"features/parent/SHOP-41","head":"c9f4b55","merged":["features/SHOP-42"],"unmerged":[]},"/w/shop/shop-web":{"base":"8fad31a3a517151a6bb69037cc95afff17c28990","branch":"features/parent/SHOP-41","head":"a672aac","merged":["features/SHOP-43"],"unmerged":[]}}"# },
    StoryRow { note: "장난감 zz-mr-old 스토리 SHOP-31(옛 꼴)", story: "toy:zz-mr-old:SHOP-31", repos: r#"{"/w/zz-mr-toys/solo":{"base":"695135148dec74c4cd5647391f6e5225380f3924","branch":"features/parent/SHOP-31","head":"c1ceaa4","merged":["features/SHOP-32","features/SHOP-33"],"unmerged":[]}}"# },
];

// 입력이 객체가 아니면 빈 객체 — Swift `object(_:)`
fn object(json: &str) -> Obj {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(o)) => o,
        _ => Obj::new(),
    }
}

fn toy(id: &str) -> &'static str {
    TOYS.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or("")
}

// 승격 차례 줄임 글자: "autopilot>dev autopilot>live@dev live>master@live"(@ 뒤는 after) — Swift `steps(_:)`
fn steps(s: &str) -> Vec<Step> {
    s.split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (ft, after) = match t.split_once('@') {
                Some((a, b)) => (a, Some(b)),
                None => (t, None),
            };
            let (from, into) = ft.split_once('>').expect("from>into");
            Step::new(from, into, after)
        })
        .collect()
}

#[test]
fn 표_길이() {
    assert_eq!(TOYS.len(), 26, "Swift 장난감 표 줄 수");
    assert_eq!(OPEN_ROWS.len(), 108, "Swift openRows 줄 수");
    assert_eq!(STORY_ROWS.len(), 33, "Swift storyRows 줄 수");
    assert_eq!(mark_rows().len(), 51, "Swift markRows 줄 수");
    assert_eq!(discard_table().len(), 10, "Swift discardRows 줄 수");
    assert_eq!(CARRIES_ROWS.len(), 8, "Swift carriesRows 줄 수");
    assert_eq!(FINISH_ROOT_ROWS.len(), 6, "Swift finishRootRows 줄 수");
}

// Swift: MergeOpenTests.표 — 버리지도 마지막 자율주행 단계에 합치지도 않은 합칠 쪽, 또는 안 한 마지막 기존 가지 단계가 있으면 남았다
#[test]
fn merge_open_표() {
    for r in OPEN_ROWS {
        let br = match r.branches.strip_prefix("toy:") {
            Some(id) => object(toy(id)),
            None => object(r.branches),
        };
        assert!(!br.is_empty() || r.branches == "{}", "{}: 장난감 기록을 찾지 못했다", r.note);
        assert_eq!(merge_open(&br, &steps(r.steps), r.with_autopilot), r.open, "{}", r.note);
    }
}


// Swift: MergeMarkTests.표 — 가지 기록 없음은 nil, 합칠 쪽마다 버림·합침·기다림 — 모두 같으면 그것(합침은 마지막으로 받은 가지), 섞이면 섞인 수
#[test]
fn merge_mark_표() {
    for r in mark_rows() {
        let br = match r.branches.strip_prefix("toy:") {
            Some(id) => object(toy(id)),
            None => object(r.branches),
        };
        assert!(!br.is_empty(), "{}: 장난감 기록을 찾지 못했다", r.note);
        let m = merge_mark(&br, r.with_autopilot);
        assert_eq!(m, r.mark, "{}", r.note);
        assert_eq!(m.map(|m| m.label()).as_deref(), r.label, "{}", r.note);
    }
}

// Swift: StoryReposTests.표 — 새 꼴 repos 는 저장소마다, 옛 꼴 repo 는 그 저장소 하나로
#[test]
fn story_repos_표() {
    for r in STORY_ROWS {
        let v: Option<Value> = if let Some(rest) = r.story.strip_prefix("toy:") {
            let (id, key) = rest.split_once(':').expect("toy:<id>:<key>");
            let found = object(toy(id)).get("stories").and_then(|s| s.get(key)).cloned();
            assert!(found.is_some(), "{}: 장난감 기록을 찾지 못했다", r.note);
            found
        } else if r.story == "nil" {
            None
        } else {
            serde_json::from_str(r.story).ok()
        };
        let want: Value = serde_json::from_str(r.repos).expect("repos JSON");
        assert_eq!(serde_json::to_value(story_repos(v.as_ref())).unwrap(), want, "{}", r.note);
    }
}


// Swift: DiscardRowsTests.표 — 지운 가지는 버린 줄, 지우지 못한 가지는 못 한 줄
#[test]
fn discard_rows_표() {
    for r in discard_table() {
        let (d, f) = discard_rows("story:S1", "/r/a", "f/S1", &r.fate, r.is_source, r.recorded, 5.0);
        let want = |s: Option<&str>| s.map(|j| serde_json::from_str::<Value>(j).expect("JSON"));
        assert_eq!(d.map(Value::Object), want(r.discarded), "{}", r.note);
        assert_eq!(f.map(Value::Object), want(r.failed), "{}", r.note);
    }
}

// Swift: CarriesBaseTests.표 — 이 실행의 가지를 합치는 단계에서 앞 단계 가지가 출발 가지면 센다
#[test]
fn carries_base_표() {
    for r in CARRIES_ROWS {
        assert_eq!(carries_base(r.source, r.after, r.base), r.want, "{}", r.note);
    }
}

// Swift: FinishRootTests.표 — 뿌리는 그 실행이 출발한 뿌리, --dev 는 맞는지만 본다
#[test]
fn finish_root_표() {
    for r in FINISH_ROOT_ROWS {
        let (dev, refuse) = finish_root(r.flag, r.run_dev);
        assert_eq!(dev, r.dev, "{}", r.note);
        assert_eq!(refuse, r.refuse, "{}", r.note);
    }
}

// ---- 추가: Swift 시험에 없는 직접 호출·경계·Windows 경로 ----

#[test]
fn extra_row_literals_match_swift_source() {
    // 한글·「」·슬래시·# 이 든 글자가 옮겨지며 변하지 않았는지 몇 줄을 글자째 견준다
    let r = OPEN_ROWS.iter().find(|r| r.note.starts_with("한 저장소만 합치고")).expect("row");
    assert_eq!(r.note, "한 저장소만 합치고 다른 저장소는 못 함(failed) → 단계가 끝나지 않음");
    assert_eq!(r.steps, "autopilot>live");
    let s = STORY_ROWS.iter().find(|r| r.note.contains("zz-ap52-jira 스토리 SHOP-12")).expect("row");
    assert!(s.repos.contains("건너뜀: 기대는 앞 단위 「품절 상품이 합계에 들어가는 문제 고치기」 가 통과하지 못해 시작하지 않습니다."));
    assert!(s.repos.contains(r#""branch":"fix/_SHOP-12/parent/SHOP-12""#));
    assert_eq!(toy("zz-ap51-inplace"), r#"{"repos":{"/w/zz-ap51-toys/shop":{}},"stories":{}}"#);
}

#[test]
fn extra_rows_read_numbers_like_nsnumber() {
    let br = object(r#"{"promoted":[{"at":10.5},{"at":"7"},{"at":true},{"at":null},{}]}"#);
    let at: Vec<Option<f64>> = promoted_rows(&br).iter().map(|p| p.at).collect();
    assert_eq!(at, vec![Some(10.5), None, Some(1.0), None, None]);
}

#[test]
fn extra_row_fields_that_are_not_strings_read_as_empty() {
    let br = object(r#"{"promoted":[{"source":1,"repo":"/r","into":null,"how":"merge","to":"abc"}],"failed":[{"op":"merge","why":"충돌","at":3}]}"#);
    assert_eq!(
        promoted_rows(&br),
        vec![Promoted { repo: "/r".into(), how: "merge".into(), to: "abc".into(), ..Default::default() }]
    );
    let f = failed_rows(&br);
    assert_eq!((f[0].op.as_str(), f[0].why.as_str(), f[0].at), ("merge", "충돌", Some(3.0)));
}

#[test]
fn extra_array_with_a_non_object_item_is_empty_as_a_whole() {
    let br = object(r#"{"promoted":[{"source":"autopilot","into":"dev"},3],"failed":[{"op":"merge"},"x"],"discarded":[{"source":"autopilot"},null]}"#);
    assert!(promoted_rows(&br).is_empty());
    assert!(failed_rows(&br).is_empty());
    // 버린 줄도 같다: 전체가 빈 목록이라 자율주행 가지가 남는다
    assert!(merge_open(&br, &[], true));
    assert!(promoted_rows(&object(r#"{"promoted":{"source":"autopilot"}}"#)).is_empty());
}

#[test]
fn extra_unresolved_filters_by_source_and_ignores_other_ops() {
    let br = object(
        r#"{"failed":[{"at":5,"into":"live","op":"merge","repo":"/r/a","source":"story:S1"},
                      {"at":6,"into":"live","op":"promote","repo":"/r/b","source":"autopilot"},
                      {"at":7,"into":"live","op":"discard","repo":"/r/c","source":"autopilot"},
                      {"at":8,"into":"dev","op":"merge","repo":"/r/d","source":"autopilot"}],
           "promoted":[{"at":9,"into":"live","repo":"/r/a","source":"story:S1"}]}"#,
    );
    let (p, f) = (promoted_rows(&br), failed_rows(&br));
    let all = unresolved(&p, &f, None, "live");
    assert_eq!(all.iter().map(|x| x.repo.as_str()).collect::<Vec<_>>(), vec!["/r/b"]);
    assert!(unresolved(&p, &f, Some("story:S1"), "live").is_empty());
    assert_eq!(unresolved(&p, &f, Some("autopilot"), "dev").len(), 1);
    assert!(unresolved(&p, &f, Some("story:S1"), "dev").is_empty());
}

#[test]
fn extra_step_done_needs_a_promoted_line_and_no_leftover() {
    let br = object(r#"{"promoted":[{"at":10,"into":"live","repo":"/r/a","source":"autopilot"}]}"#);
    let (p, f) = (promoted_rows(&br), failed_rows(&br));
    assert!(step_done(&p, &f, Some("autopilot"), "live"));
    assert!(step_done(&p, &f, None, "live"));
    assert!(!step_done(&p, &f, Some("story:S1"), "live"));
    assert!(!step_done(&p, &f, Some("autopilot"), "dev"));
    assert!(!step_done(&[], &[], None, "live"));
}

#[test]
fn extra_merge_sources_order_head_and_non_object_stories() {
    let br = object(r#"{"stories":{"S2":{"repos":{"/r":{"head":"h"}}},"S10":{"repos":{"/r":{"head":"h"}}},"S1":{"repos":{"/r":{"branch":"b"}}}}}"#);
    assert_eq!(merge_sources(&br, false), vec!["story:S10", "story:S2"]);
    assert_eq!(merge_sources(&br, true), vec!["autopilot", "story:S10", "story:S2"]);
    // head 가 null 이어도 칸이 있으면 가지를 만든 것으로 본다(Swift `!= nil`, NSNull 은 nil 이 아니다)
    let br = object(r#"{"stories":{"S1":{"repos":{"/r":{"head":null}}}}}"#);
    assert_eq!(merge_sources(&br, false), vec!["story:S1"]);
    assert_eq!(merge_sources(&object(r#"{"stories":["S1"]}"#), false), vec!["autopilot"]);
}

#[test]
fn extra_story_repos_old_form_does_not_touch_the_input() {
    let v = json!({"repo": "/r/a", "branch": "f/S1"});
    let got = story_repos(Some(&v));
    assert_eq!(got["/r/a"].get("repo"), None);
    assert_eq!(v["repo"], "/r/a");
    assert!(story_repos(Some(&json!({"repo": 7}))).is_empty());
}

#[test]
fn extra_windows_repo_paths_are_kept_verbatim() {
    let br = object(
        r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"C:\\w\\b","source":"autopilot"}],
            "promoted":[{"at":10,"into":"live","repo":"C:\\w\\a","source":"autopilot"},{"at":12,"into":"live","repo":"c:\\w\\b","source":"autopilot"}]}"#,
    );
    // 대소문자가 다른 경로는 다른 저장소다 — 규칙은 경로를 고치지 않고 글자째 견준다
    assert!(merge_open(&br, &steps("autopilot>live"), true));
    let br = object(
        r#"{"failed":[{"at":11,"into":"live","op":"merge","repo":"C:\\w\\b","source":"autopilot"}],
            "promoted":[{"at":10,"into":"live","repo":"C:\\w\\a","source":"autopilot"},{"at":12,"into":"live","repo":"C:\\w\\b","source":"autopilot"}]}"#,
    );
    assert!(!merge_open(&br, &steps("autopilot>live"), true));
    let v = json!({"repos": {"C:\\w\\a": {"head": "h"}, "/mnt/c/w/b": {"head": "h"}}});
    assert_eq!(story_repos(Some(&v)).keys().map(String::as_str).collect::<Vec<_>>(), vec!["/mnt/c/w/b", "C:\\w\\a"]);
}

#[test]
fn extra_merge_mark_label_and_has_waiting() {
    assert_eq!(MergeMark::Promoted("main".to_string()).label(), "main 까지 올림");
    assert_eq!(MergeMark::Promoted("Main".to_string()).label(), "Main 에 합침");
    assert_eq!(MergeMark::Mixed { promoted: 0, discarded: 0, waiting: 3 }.label(), "기다림 3");
    assert_eq!(MergeMark::Mixed { promoted: 2, discarded: 0, waiting: 1 }.label(), "합침 2 · 기다림 1");
    assert_eq!(MergeMark::Mixed { promoted: 0, discarded: 0, waiting: 0 }.label(), "");
    assert!(MergeMark::Waiting.has_waiting());
    assert!(MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 1 }.has_waiting());
    assert!(!MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 0 }.has_waiting());
    assert!(!MergeMark::Discarded.has_waiting());
    assert!(!MergeMark::Promoted("live".to_string()).has_waiting());
}

#[test]
fn extra_merge_mark_empty_into_counts_as_last_text_but_not_as_promoted() {
    // `into` 가 빈 글자인 줄은 합친 줄로는 세지 않지만(source 집합에서 빠짐) 원본 compactMap 은 빈 글자도 글자로 본다
    let br = object(r#"{"promoted":[{"into":"live","source":"autopilot"},{"into":"","source":"autopilot"}],"repos":{"/r/a":{}}}"#);
    assert_eq!(merge_mark(&br, true), Some(MergeMark::Promoted(String::new())));
    // promoted 가 배열이 아니면 줄이 없다 → 합친 줄 없음 → 기다림
    let br = object(r#"{"promoted":"x","repos":{"/r/a":{}}}"#);
    assert_eq!(merge_mark(&br, true), Some(MergeMark::Waiting));
}

#[test]
fn extra_merge_mark_promoted_and_waiting_without_discard_is_mixed() {
    // Swift 표에 없는 조합: 한 스토리는 합치고 한 스토리는 기다림(버림 없음) → 섞임이지 「마지막으로 받은 가지」가 아니다
    let br = object(
        r#"{"promoted":[{"into":"live","repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#,
    );
    let m = merge_mark(&br, false).expect("mark");
    assert_eq!(m, MergeMark::Mixed { promoted: 1, discarded: 0, waiting: 1 });
    assert_eq!(m.label(), "합침 1 · 기다림 1");
    assert!(m.has_waiting());
}

#[test]
fn extra_discard_rows_write_the_time_like_foundation() {
    let (d, _) = discard_rows("autopilot", r"C:\w\a", "b", &RefFate::Deleted { head: "h".to_string() }, false, false, 5.0);
    assert_eq!(serde_json::to_string(&Value::Object(d.unwrap())).unwrap(), r#"{"source":"autopilot","repo":"C:\\w\\a","branch":"b","head":"h","at":5}"#);
    let (d, _) = discard_rows("autopilot", "/r", "b", &RefFate::Missing, true, false, 5.5);
    assert_eq!(d.unwrap().get("at"), Some(&json!(5.5)));
    // 못 한 줄은 op 가 맨 앞이고 why 가 있다
    let (d, f) = discard_rows("autopilot", "/r", "b", &RefFate::Failed("x".to_string()), true, true, 1.0);
    assert!(d.is_none());
    assert_eq!(Value::Object(f.unwrap()), json!({"op":"discard","source":"autopilot","repo":"/r","branch":"b","why":"x","at":1}));
}

#[test]
fn extra_carries_base_needs_equal_after_and_base() {
    assert!(!carries_base("autopilot", None, None));
    assert!(carries_base("autopilot", Some("dev"), Some("dev")));
    assert!(!carries_base("branch:x", Some("dev"), Some("dev")));
    assert!(carries_base("story:S1", Some(""), Some("")));
}

// ---- 후반 표(Swift 시험 파일에서 기계로 옮김) ----

fn mark_rows() -> Vec<MarkRow> {
    vec![
        MarkRow { note: "repos 가 없으면 가지 기록 없는 실행(nil)", branches: r#"{"promoted":[{"into":"dev","source":"autopilot"}]}"#, mark: None, label: None, with_autopilot: true },
        MarkRow { note: "repos 가 비었으면 버린 줄이 있어도 nil", branches: r#"{"discarded":[{"source":"autopilot"}],"repos":{}}"#, mark: None, label: None, with_autopilot: true },
        MarkRow { note: "repos 가 객체가 아니면 nil", branches: r#"{"repos":["/r/a"]}"#, mark: None, label: None, with_autopilot: true },
        MarkRow { note: "가지는 있고 합친 줄·버린 줄이 없으면 합치기 기다림", branches: r#"{"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "빈 promoted·discarded 도 합치기 기다림", branches: r#"{"discarded":[],"promoted":[],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "버린 줄이 있으면 버림", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "5-3b ②: 합칠 쪽이 아닌 버린 줄(스토리 기록이 없는 story:S1)은 세지 않는다 → master 까지 올림(전에는 실행 전체가 버림)", branches: r#"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"into":"master","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("master".to_string())), label: Some("master 까지 올림"), with_autopilot: true },
        MarkRow { note: "5-3b ②: 버린 줄은 source 가 있는 객체만 센다(전에는 무엇이든 하나 있으면 버림)", branches: r#"{"discarded":["x"],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "5-3b ②: 한 스토리는 live 에 합치고 한 스토리는 버림 → 섞임", branches: r#"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"into":"live","repo":"/r/a","source":"story:S2"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, mark: Some(MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 0 }), label: Some("합침 1 · 버림 1"), with_autopilot: false },
        MarkRow { note: "5-3b ②: 합침·버림·기다림이 섞임", branches: r#"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"promoted":[{"into":"dev","repo":"/r/a","source":"story:S2"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}},"S3":{"repos":{"/r/a":{"branch":"f/S3","head":"h3"}}}}}"#, mark: Some(MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 1 }), label: Some("합침 1 · 버림 1 · 기다림 1"), with_autopilot: false },
        MarkRow { note: "5-3b ②: 버림과 기다림만", branches: r#"{"discarded":[{"repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, mark: Some(MergeMark::Mixed { promoted: 0, discarded: 1, waiting: 1 }), label: Some("버림 1 · 기다림 1"), with_autopilot: false },
        MarkRow { note: "5-3b ②: 모든 스토리를 합침 → 마지막으로 받은 가지(기존 가지끼리 올린 master 까지)", branches: r#"{"promoted":[{"into":"live","repo":"/r/a","source":"story:S1"},{"into":"live","repo":"/r/a","source":"story:S2"},{"into":"master","repo":"/r/a","source":"branch:live"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, mark: Some(MergeMark::Promoted("master".to_string())), label: Some("master 까지 올림"), with_autopilot: false },
        MarkRow { note: "5-3b ②: Jira 단위만 있는 실행의 쓰이지 않은 자율주행 가지를 지운 줄은 세지 않는다", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}],"promoted":[{"into":"live","repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}}}}"#, mark: Some(MergeMark::Promoted("live".to_string())), label: Some("live 에 합침"), with_autopilot: false },
        MarkRow { note: "5-3b ②: 스토리 없는 단위가 있는 Jira 실행은 자율주행 가지도 합칠 쪽 — 그 가지만 버림 → 섞임", branches: r#"{"discarded":[{"repo":"/r/a","source":"autopilot"}],"promoted":[{"into":"live","repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1"}}}}}"#, mark: Some(MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 0 }), label: Some("합침 1 · 버림 1"), with_autopilot: true },
        MarkRow { note: "5-3b ②④: 가지를 만들지 않은 스토리는 세지 않는다", branches: r#"{"promoted":[{"into":"live","repo":"/r/a","source":"story:S2"}],"repos":{"/r/a":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1"}}},"S2":{"repos":{"/r/a":{"branch":"f/S2","head":"h2"}}}}}"#, mark: Some(MergeMark::Promoted("live".to_string())), label: Some("live 에 합침"), with_autopilot: false },
        MarkRow { note: "5-3b ④: 보드 밖에서 지운 가지를 버린 것으로 적은 줄(left missing)도 버림", branches: r#"{"discarded":[{"branch":"autopilot/x","left":"missing","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "discarded 가 배열이 아니면 버린 것 없음", branches: r#"{"discarded":{"source":"autopilot"},"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "dev 에 합침", branches: r#"{"promoted":[{"into":"dev","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("dev".to_string())), label: Some("dev 에 합침"), with_autopilot: true },
        MarkRow { note: "마지막으로 받은 가지가 master 면 까지 올림", branches: r#"{"promoted":[{"into":"dev","repo":"/r/a","source":"autopilot"},{"into":"master","repo":"/r/a","source":"branch:dev"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("master".to_string())), label: Some("master 까지 올림"), with_autopilot: true },
        MarkRow { note: "main 도 까지 올림", branches: r#"{"promoted":[{"into":"main","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("main".to_string())), label: Some("main 까지 올림"), with_autopilot: true },
        MarkRow { note: "마지막은 promoted 의 차례로 고른다(시각이 아니다)", branches: r#"{"promoted":[{"at":20,"into":"master","repo":"/r/a","source":"autopilot"},{"at":10,"into":"dev","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("dev".to_string())), label: Some("dev 에 합침"), with_autopilot: true },
        MarkRow { note: "into 가 없는 줄은 건너뛰고 그 앞 줄", branches: r#"{"promoted":[{"into":"live","repo":"/r/a","source":"autopilot"},{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("live".to_string())), label: Some("live 에 합침"), with_autopilot: true },
        MarkRow { note: "into 가 있는 줄이 없으면 합치기 기다림", branches: r#"{"promoted":[{"repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "가지 이름은 대소문자를 가린다(Master 는 에 합침)", branches: r#"{"promoted":[{"into":"Master","repo":"/r/a","source":"autopilot"}],"repos":{"/r/a":{}}}"#, mark: Some(MergeMark::Promoted("Master".to_string())), label: Some("Master 에 합침"), with_autopilot: true },
        MarkRow { note: "여러 저장소 스토리: 버린 줄이 있는 스토리는 합친 줄이 있어도 버림(엔진은 합친 뒤 버리지 않아 생기지 않는 꼴)", branches: r#"{"discarded":[{"repo":"/r/b","source":"story:S1"}],"promoted":[{"into":"dev","repo":"/r/a","source":"story:S1"},{"into":"live","repo":"/r/a","source":"story:S1"}],"repos":{"/r/a":{},"/r/b":{}},"stories":{"S1":{"repos":{"/r/a":{"branch":"f/S1","head":"h1","merged":["T1","T2"]},"/r/b":{"branch":"f/S1","unmerged":["T3"]}}}}}"#, mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: false },
        MarkRow { note: "장난감 20261027-1909-zz-ap52-todo(저장소 1·버린 줄 1)", branches: "toy:20261027-1909-zz-ap52-todo", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 20261027-1918-zz-ap52-jira(저장소 1·스토리 3·버린 줄 7) · 스토리 셋 모두 버림", branches: "toy:20261027-1918-zz-ap52-jira", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: false },
        MarkRow { note: "장난감 20261028-1043-zz-ap53-jira(저장소 1·스토리 2·합친 줄 3·버린 줄 3) · 5-3b ②: SHOP-12 는 live·master 까지, SHOP-20 은 버림(전에는 버림)", branches: "toy:20261028-1043-zz-ap53-jira", mark: Some(MergeMark::Mixed { promoted: 1, discarded: 1, waiting: 0 }), label: Some("합침 1 · 버림 1"), with_autopilot: false },
        MarkRow { note: "장난감 merge-check-1(저장소 1·버린 줄 1)", branches: "toy:merge-check-1", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap51-inplace(저장소 1)", branches: "toy:zz-ap51-inplace", mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap51-jira(저장소 1·스토리 2) · 두 스토리 모두 기다림", branches: "toy:zz-ap51-jira", mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: false },
        MarkRow { note: "장난감 zz-ap51-judge(저장소 1)", branches: "toy:zz-ap51-judge", mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap51-long(저장소 1)", branches: "toy:zz-ap51-long", mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap51-wt(저장소 1)", branches: "toy:zz-ap51-wt", mark: Some(MergeMark::Waiting), label: Some("합치기 기다림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap52-p1(저장소 1·버린 줄 1)", branches: "toy:zz-ap52-p1", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap52-pj(저장소 1·스토리 3·버린 줄 8) · 스토리 셋 모두 버림", branches: "toy:zz-ap52-pj", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: false },
        MarkRow { note: "장난감 zz-ap53-cpu(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-cpu", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap53-inplace(저장소 2·합친 줄 6)", branches: "toy:zz-ap53-inplace", mark: Some(MergeMark::Promoted("master".to_string())), label: Some("master 까지 올림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap53-jira(저장소 1·스토리 3·합친 줄 5·버린 줄 3) · 5-3b ②: SHOP-16·SHOP-12 는 live·master 까지, SHOP-20 은 버림(전에는 버림)", branches: "toy:zz-ap53-jira", mark: Some(MergeMark::Mixed { promoted: 2, discarded: 1, waiting: 0 }), label: Some("합침 2 · 버림 1"), with_autopilot: false },
        MarkRow { note: "장난감 zz-ap53-wt(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-wt", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap53-wt2(저장소 1·합친 줄 3)", branches: "toy:zz-ap53-wt2", mark: Some(MergeMark::Promoted("master".to_string())), label: Some("master 까지 올림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-ap53-wt3(저장소 1·버린 줄 1)", branches: "toy:zz-ap53-wt3", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-cpu-ap(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-ap", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-cpu-ap2(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-ap2", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-cpu-run(저장소 1·버린 줄 1)", branches: "toy:zz-cpu-run", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-merge-gui(저장소 1·버린 줄 1)", branches: "toy:zz-merge-gui", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: true },
        MarkRow { note: "장난감 zz-mr-before(저장소 2·스토리 3·버린 줄 10) · 스토리 셋 모두 버림", branches: "toy:zz-mr-before", mark: Some(MergeMark::Discarded), label: Some("버림"), with_autopilot: false },
        MarkRow { note: "장난감 zz-mr-jira(저장소 2·스토리 3·합친 줄 7·못 한 줄 1·버린 줄 6) · 5-3b ②: SHOP-1 은 두 저장소 live·master 까지, SHOP-4·SHOP-6 은 버림(전에는 버림)", branches: "toy:zz-mr-jira", mark: Some(MergeMark::Mixed { promoted: 1, discarded: 2, waiting: 0 }), label: Some("합침 1 · 버림 2"), with_autopilot: false },
        MarkRow { note: "장난감 zz-mr-jira2(저장소 2·스토리 3·합친 줄 5·못 한 줄 1·버린 줄 8) · 5-3b ②: SHOP-11 은 두 저장소 live 까지, SHOP-14·SHOP-21 은 버림(전에는 버림)", branches: "toy:zz-mr-jira2", mark: Some(MergeMark::Mixed { promoted: 1, discarded: 2, waiting: 0 }), label: Some("합침 1 · 버림 2"), with_autopilot: false },
        MarkRow { note: "장난감 zz-mr-jira3(저장소 2·스토리 1·합친 줄 4·버린 줄 2) · 5-3b ②: SHOP-41 을 두 저장소 live 에 합침, 버린 줄은 쓰이지 않은 자율주행 가지뿐(전에는 버림)", branches: "toy:zz-mr-jira3", mark: Some(MergeMark::Promoted("live".to_string())), label: Some("live 에 합침"), with_autopilot: false },
        MarkRow { note: "장난감 zz-mr-old(저장소 1·스토리 1·합친 줄 2·버린 줄 1) · 5-3b ②: SHOP-31 을 live 에 합침, 버린 줄은 쓰이지 않은 자율주행 가지뿐(전에는 버림)", branches: "toy:zz-mr-old", mark: Some(MergeMark::Promoted("live".to_string())), label: Some("live 에 합침"), with_autopilot: false },
    ]
}

fn discard_table() -> Vec<DiscardRow> {
    vec![
        DiscardRow { note: "지운 합칠 쪽 가지 → 버린 줄", fate: RefFate::Deleted { head: "abc1234".to_string() }, is_source: true, recorded: false, discarded: Some(r#"{"at":5,"branch":"f/S1","head":"abc1234","repo":"/r/a","source":"story:S1"}"#), failed: None },
        DiscardRow { note: "지운 작업 가지도 버린 줄", fate: RefFate::Deleted { head: "def5678".to_string() }, is_source: false, recorded: false, discarded: Some(r#"{"at":5,"branch":"f/S1","head":"def5678","repo":"/r/a","source":"story:S1"}"#), failed: None },
        DiscardRow { note: "지우지 못함 → 못 한 줄", fate: RefFate::Failed("잠금 파일".to_string()), is_source: true, recorded: false, discarded: None, failed: Some(r#"{"at":5,"branch":"f/S1","op":"discard","repo":"/r/a","source":"story:S1","why":"잠금 파일"}"#) },
        DiscardRow { note: "5-3b ④: 이미 없는 합칠 쪽 가지(보드 밖에서 지움)도 버린 줄로 적는다(전에는 적지 않아 합치기 기다림이 남음)", fate: RefFate::Missing, is_source: true, recorded: false, discarded: Some(r#"{"at":5,"branch":"f/S1","left":"missing","repo":"/r/a","source":"story:S1"}"#), failed: None },
        DiscardRow { note: "5-3b ④: 같은 이름으로 다시 만든 합칠 쪽 가지는 남기되 버린 줄로 적는다", fate: RefFate::Foreign, is_source: true, recorded: false, discarded: Some(r#"{"at":5,"branch":"f/S1","left":"foreign","repo":"/r/a","source":"story:S1"}"#), failed: None },
        DiscardRow { note: "같은 이름으로 다시 만든 작업 가지는 적지 않는다", fate: RefFate::Foreign, is_source: false, recorded: false, discarded: None, failed: None },
        DiscardRow { note: "5-3b ④: 이미 버린 줄이 있는 가지가 없으면 다시 적지 않는다(버리기를 다시 눌러도 줄이 늘지 않게)", fate: RefFate::Missing, is_source: true, recorded: true, discarded: None, failed: None },
        DiscardRow { note: "이미 버린 줄이 있는 남긴 가지도 다시 적지 않는다", fate: RefFate::Foreign, is_source: true, recorded: true, discarded: None, failed: None },
        DiscardRow { note: "이미 버린 줄이 있어도 지운 가지는 적는다(같은 이름으로 다시 생겼다가 지움)", fate: RefFate::Deleted { head: "abc1234".to_string() }, is_source: true, recorded: true, discarded: Some(r#"{"at":5,"branch":"f/S1","head":"abc1234","repo":"/r/a","source":"story:S1"}"#), failed: None },
        DiscardRow { note: "이미 없는 작업 가지는 적지 않는다", fate: RefFate::Missing, is_source: false, recorded: false, discarded: None, failed: None },
    ]
}

const CARRIES_ROWS: &[CarriesRow] = &[
    CarriesRow { note: "② live ← 자율주행 가지(after dev), 출발 가지 dev → 센다", source: "autopilot", after: Some("dev"), base: Some("dev"), want: true },
    CarriesRow { note: "스토리 가지도 같다", source: "story:S1", after: Some("dev"), base: Some("dev"), want: true },
    CarriesRow { note: "① dev ← 자율주행 가지(after 없음) → 세지 않는다", source: "autopilot", after: None, base: Some("dev"), want: false },
    CarriesRow { note: "② 인데 출발 가지가 live → 세지 않는다", source: "autopilot", after: Some("dev"), base: Some("live"), want: false },
    CarriesRow { note: "출발 가지를 모름 → 세지 않는다", source: "autopilot", after: Some("dev"), base: None, want: false },
    CarriesRow { note: "5-3b ③: ③ master ← live(after live)는 출발 가지가 live 여도 세지 않는다(전에는 앞 실행의 커밋까지 셈)", source: "branch:live", after: Some("live"), base: Some("live"), want: false },
    CarriesRow { note: "기존 가지끼리 올리는 단계는 출발 가지와 상관없이 세지 않는다", source: "branch:dev", after: Some("dev"), base: Some("dev"), want: false },
    CarriesRow { note: "③ 인데 출발 가지가 dev → 세지 않는다", source: "branch:live", after: Some("live"), base: Some("dev"), want: false },
];

const FINISH_ROOT_ROWS: &[FinishRootRow] = &[
    FinishRootRow { note: "시험 빌드 실행에 --dev → 시험 빌드 뿌리", flag: true, run_dev: Some(true), dev: true, refuse: false },
    FinishRootRow { note: "시험 빌드 실행에 --dev 가 없어도 run.json 을 따른다(보드·옛 손 명령)", flag: false, run_dev: Some(true), dev: true, refuse: false },
    FinishRootRow { note: "설치판 실행 → 설치판 뿌리", flag: false, run_dev: Some(false), dev: false, refuse: false },
    FinishRootRow { note: "설치판 실행에 --dev → 거부(다른 뿌리의 승격 차례·기록으로 가지를 움직이지 않게)", flag: true, run_dev: Some(false), dev: false, refuse: true },
    FinishRootRow { note: "run.json 에 dev 가 없는 옛 실행은 --dev 를 따른다", flag: true, run_dev: None, dev: true, refuse: false },
    FinishRootRow { note: "옛 실행에 --dev 가 없으면 설치판 뿌리", flag: false, run_dev: None, dev: false, refuse: false },
];
