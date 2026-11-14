//! 단위 커밋 기록과 승격 뒤 되돌림 신호. 원본: `Tests/AutopilotRulesTests/UnitCommitsTests.swift`(260줄, 1:1).
//!
//! 가짜 git(저장소별 승격 뒤 커밋 목록·병합이 들여온 커밋)으로 되돌림 줄 꼴(보통·병합·되돌림의 되돌림·짧은 이름·다른 저장소의 같은 이름·사람이 고쳐 쓴 메시지)을 본다.
//! 기록 저장소 판 5(unit_commits) 올리기, tickets 표 줄, 빠진 단위 보고서도 여기. 값은 장난감(zz-toy·TOY-1·/w/…)

use claude_board_autopilot::rules::formats::grapheme_len;
use claude_board_autopilot::rules::records::*;
use claude_board_autopilot::rules::unit_commits::*;
use rusqlite::Connection;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

/// `sha(_ c:_:)` — 글자 `c` 를 40자로 반복한 이름(시험용 꼴 맞는 이름)
fn sha(c: char) -> String {
    (0..40).map(|_| c).collect()
}

/// 이름 앞 `n` 자(Swift `prefix(n)`)
fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// json! 꼴을 `Obj`(사전)로 — Swift 사전 리터럴이 `Any` 사전인 자리의 대응
fn obj(v: serde_json::Value) -> Obj {
    v.as_object().cloned().expect("사전 꼴 자료")
}

/// 가짜 git — 원본: `FakeRevertGit: APRevertWorld`(클래스라 `calls` 를 몰래 적는다 → `RefCell` 로 내부 가변)
struct FakeRevertGit {
    logs: HashMap<String, Vec<ApScannedCommit>>, // "<repo>|<into>|<from>" → 새것부터
    merges: HashMap<String, Vec<String>>,        // "<repo>|<merge>" → 들여온 커밋
    calls: RefCell<Vec<String>>,
}

impl FakeRevertGit {
    fn new() -> Self {
        FakeRevertGit { logs: HashMap::new(), merges: HashMap::new(), calls: RefCell::new(Vec::new()) }
    }
}

impl ApRevertWorld for FakeRevertGit {
    fn commits(&self, repo: &str, into: &str, from: &str, max: usize) -> Option<Vec<ApScannedCommit>> {
        self.calls.borrow_mut().push(format!("log {repo} {into} {}", short(from, 4)));
        self.logs.get(&format!("{repo}|{into}|{from}")).map(|l| l.iter().take(max).cloned().collect())
    }
    fn introduced(&self, repo: &str, merge: &str, _max: usize) -> Option<Vec<String>> {
        self.calls.borrow_mut().push(format!("introduced {repo} {}", short(merge, 4)));
        self.merges.get(&format!("{repo}|{merge}")).cloned()
    }
}

// 자율주행 단위 커밋 기록

#[test]
fn 단위_기록의_commits_merge_와_run_json_promoted_에서_줄로() {
    // 단위 기록의 commits·merge 와 run.json promoted 에서 줄로(앞으로 감기·그대로는 새 커밋이 없어 남기지 않음)
    let e = serde_json::json!({
        "ended": 200.0,
        "commits": [
            {"repo": "/w/shop", "branch": "features/TOY-2", "shas": [sha('a'), sha('b'), "zz", sha('a')]}
        ],
        "merge": {"state": "merged", "how": "merge", "head": sha('c'), "branch": "features/parent/TOY-1", "story": "TOY-1", "repo": "/w/shop", "at": 210.0}
    });
    let got = unit_commit_rows("r1", 2, &obj(e.clone()));
    let seen: Vec<String> = got.iter().map(|r| format!("{}:{}:{}", r.kind.raw_value(), short(&r.sha, 1), r.r#ref)).collect();
    assert_eq!(seen, ["unit:a:features/TOY-2", "unit:b:features/TOY-2", "story_merge:c:features/parent/TOY-1"]);
    assert_eq!(got[2].source.as_deref(), Some("story:TOY-1"));
    assert_eq!(got[2].at, 210.0);
    assert_eq!(got[0].at, 200.0);
    assert!(got.iter().all(|r| r.unit_n == Some(2)));
    // 앞으로 감기로 합침·옛 기록(merge.repo 없음)은 단위 커밋의 저장소가 하나일 때 그것
    let mut ff = e.clone();
    ff["merge"] = serde_json::json!({"state": "merged", "how": "ff", "head": sha('b')});
    assert!(unit_commit_rows("r1", 2, &obj(ff)).into_iter().all(|r| r.kind != ApCommitKind::StoryMerge));
    let mut old = e;
    old["merge"] = serde_json::json!({"state": "merged", "how": "merge", "head": sha('c'), "branch": "s", "story": "TOY-1"});
    let old_rows = unit_commit_rows("r1", 2, &obj(old));
    assert_eq!(old_rows.last().map(|r| r.repo.as_str()), Some("/w/shop"));
    let promoted = [
        serde_json::json!({"source": "autopilot", "repo": "/w/shop", "branch": "autopilot/r1", "into": "dev", "how": "merge", "from": sha('1'), "to": sha('d'), "at": 300.0}),
        serde_json::json!({"source": "branch:dev", "repo": "/w/shop", "branch": "dev", "into": "live", "how": "merge", "from": sha('2'), "to": sha('e'), "at": 310.0}),
        serde_json::json!({"source": "autopilot", "repo": "/w/web", "branch": "autopilot/r1", "into": "dev", "how": "ff", "from": sha('3'), "to": sha('a')}),
        serde_json::json!({"source": "story:TOY-1", "repo": "/w/shop", "into": "dev", "how": "same", "from": sha('4'), "to": sha('4')}),
    ];
    let finish = finish_commit_rows("r1", &promoted.iter().map(|v| obj(v.clone())).collect::<Vec<Obj>>());
    let fin: Vec<String> = finish.iter().map(|r| format!("{}:{}:{}", r.kind.raw_value(), r.r#ref, r.source.clone().unwrap_or_default())).collect();
    assert_eq!(fin, ["finish_merge:dev:autopilot", "promote:live:branch:dev"]);
    assert!(finish.iter().all(|r| r.unit_n.is_none()));
}

#[test]
fn 훑을_자리는_승격_기록의_받는_가지를_승격_머리_뒤로만_꼴이_틀린_줄은_뺀다() {
    let p = [
        serde_json::json!({"repo": "/w/shop", "into": "dev", "to": sha('d')}),
        serde_json::json!({"repo": "/w/shop", "into": "dev", "to": sha('d')}),
        serde_json::json!({"repo": "/w/shop", "into": "dev;rm -rf", "to": sha('d')}),
        serde_json::json!({"repo": "/w/shop", "into": "--output=/tmp/x", "to": sha('d')}),
        serde_json::json!({"repo": "/w/shop", "into": "a/../b", "to": sha('d')}),
        serde_json::json!({"repo": "rel", "into": "dev", "to": sha('d')}),
        serde_json::json!({"repo": "/w/shop", "into": "live", "to": "HEAD"}),
        serde_json::json!({"repo": "/w/shop", "into": "live", "to": sha('e')}),
    ]
    .into_iter()
    .map(obj)
    .collect::<Vec<Obj>>();
    assert_eq!(
        revert_ranges(&p),
        [ApRevertRange::new("/w/shop", "dev", &sha('d')), ApRevertRange::new("/w/shop", "live", &sha('e'))]
    );
}

// 자율주행 승격 뒤 되돌림 줄 읽기

/// 시험에서 기대하는 이름 목록을 만든다(문자열 꼴 짧게 쓰려고)
fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn 되돌림_줄_꼴() {
    let cases: [(String, Vec<String>); 12] = [
        (format!("Revert \"할 일\"\n\nThis reverts commit {}.", sha('a')), vec![sha('a')]),
        (
            format!("Revert \"Merge branch 'autopilot/r1' into dev\"\n\nThis reverts commit {}, reversing\nchanges made to {}.", sha('d'), sha('1')),
            vec![sha('d')],
        ),
        ("되돌림\n\nThis reverts commit abc1234 (할 일 고침, 2026-11-05).".to_string(), names(&["abc1234"])),
        ("되돌림\n\n  This reverts commit ABC1234DEF because it broke login".to_string(), names(&["abc1234def"])),
        ("this reverts commit abc1234.".to_string(), names(&["abc1234"])),
        ("This reverts commit abc123.".to_string(), names(&[])), // 6자는 짧음
        (format!("This reverts commit {}0.", sha('a')), names(&[])), // 41자
        ("This reverts commit abc1234x.".to_string(), names(&[])),   // 뒤가 이름 글자
        ("This reverts commit ghijklm.".to_string(), names(&[])),
        ("사람이 다시 쓴 메시지: 로그인 고침을 뺌".to_string(), names(&[])),
        ("> This reverts commit abc1234.".to_string(), names(&[])), // 인용 줄은 줄 앞이 아님
        (
            "Revert two\n\nThis reverts commit abc1234.\nThis reverts commit def5678.\nThis reverts commit abc1234.".to_string(),
            names(&["abc1234", "def5678"]),
        ),
    ];
    for (m, want) in &cases {
        assert_eq!(&revert_targets(m), want, "되돌림 줄 꼴: {m:?}");
    }
}

#[test]
fn 메시지는_앞_200줄만_본다() {
    let m = format!("{}{}", "x\n".repeat(250), "This reverts commit abc1234.");
    assert!(revert_targets(&m).is_empty());
}

// 자율주행 승격 뒤 되돌림 짝짓기

const REPO: &str = "/w/shop";

fn scan_commit(c: char, message: &str) -> ApScannedCommit {
    ApScannedCommit::new(&sha(c), message)
}

/// 원본 시험의 계산 속성 `recorded` — 되돌림 줄을 짝지을 기록된 커밋
fn recorded_list() -> Vec<ApUnitCommit> {
    vec![
        ApUnitCommit::new("r1", Some(1), REPO, &sha('a'), ApCommitKind::Unit, "autopilot/r1", None, 1.0),
        ApUnitCommit::new("r1", Some(2), REPO, &sha('b'), ApCommitKind::Unit, "autopilot/r1", None, 2.0),
        ApUnitCommit::new("r1", Some(3), REPO, &sha('f'), ApCommitKind::Unit, "autopilot/r1", None, 3.0),
        ApUnitCommit::new("r1", None, REPO, &sha('d'), ApCommitKind::FinishMerge, "dev", Some("autopilot"), 4.0),
        ApUnitCommit::new("r1", None, "/w/web", &sha('a'), ApCommitKind::Unit, "autopilot/r1", None, 5.0),
    ]
}

/// 원본 시험의 `run(_:)` — 가짜 git 에 로그·병합을 채우고 한 자리를 훑는다
fn run_scan(
    cs: Vec<ApScannedCommit>, merges: &[(String, Vec<String>)], rec: Option<Vec<ApUnitCommit>>, blocked: &[(String, String)],
) -> (ApRevertScan, FakeRevertGit) {
    let mut g = FakeRevertGit::new();
    g.logs.insert(format!("{REPO}|dev|{}", sha('d')), cs);
    for (k, v) in merges {
        g.merges.insert(format!("{REPO}|{k}"), v.clone());
    }
    (scan_reverts(&[ApRevertRange::new(REPO, "dev", &sha('d'))], &rec.unwrap_or_else(recorded_list), &g, &blocked.iter().cloned().collect()), g)
}

#[test]
fn 보통_되돌림_단위_커밋_하나를_되돌린_커밋_그_단위() {
    let (s, _) = run_scan(vec![scan_commit('9', &format!("Revert \"할 일\"\n\nThis reverts commit {}.", sha('a')))], &[], None, &[]);
    assert_eq!(
        s.hits,
        [ApRevertHit {
            run: "r1".to_string(),
            unit_n: 1,
            repo: REPO.to_string(),
            target: sha('a'),
            target_kind: ApCommitKind::Unit,
            revert: sha('9'),
            subject: "Revert \"할 일\"".to_string(),
        }]
    );
    assert!(s.withdrawn.is_empty() && s.scanned == 1);
}

#[test]
fn 병합_커밋_되돌림_그_병합이_들여온_커밋에_든_단위_모두() {
    let (s, g) = run_scan(
        vec![scan_commit('9', &format!("Revert merge\n\nThis reverts commit {}, reversing\nchanges made to {}.", sha('d'), sha('1')))],
        &[(sha('d'), vec![sha('d'), sha('a'), sha('b'), sha('7')])],
        None,
        &[],
    );
    let seen: Vec<String> =
        s.hits.iter().map(|h| format!("{}:{}:{}", h.unit_n, h.target_kind.raw_value(), short(&h.target, 1))).collect();
    assert_eq!(seen, ["1:finish_merge:d", "2:finish_merge:d"]);
    assert!(g.calls.borrow().contains(&format!("introduced {REPO} dddd")));
}

#[test]
fn 되돌림의_되돌림_다시_살림은_신호를_거두고_세_번째로_다시_되돌리면_다시_신호() {
    let r1 = scan_commit('9', &format!("Revert \"할 일\"\n\nThis reverts commit {}.", sha('a')));
    let r2 = scan_commit('8', &format!("Reapply \"할 일\"\n\nThis reverts commit {}.", sha('9')));
    let (s, _) = run_scan(vec![r2.clone(), r1.clone()], &[], None, &[]);
    let gone: Vec<String> =
        s.withdrawn.iter().map(|w| format!("{}/{}/{}", w.run, w.unit_n, short(&w.target, 1))).collect();
    assert!(s.hits.is_empty() && gone == ["r1/1/a"]);
    let r3 = scan_commit('6', &format!("Revert \"Reapply\"\n\nThis reverts commit {}.", sha('8')));
    let (s3, _) = run_scan(vec![r3, r2.clone(), r1.clone()], &[], None, &[]);
    assert_eq!(s3.hits.iter().map(|h| h.unit_n).collect::<Vec<i64>>(), [1]);
    assert_eq!(s3.hits[0].revert, sha('9'));
    assert!(s3.withdrawn.is_empty());
    // 다시 살린 뒤 같은 단위 커밋을 새로 되돌림 — 산 되돌림이 있으면 거두지 않는다
    let r4 = scan_commit('5', &format!("Revert again\n\nThis reverts commit {}.", sha('a')));
    let (s4, _) = run_scan(vec![r4, r2, r1], &[], None, &[]);
    assert_eq!(s4.hits.iter().map(|h| h.revert.clone()).collect::<Vec<String>>(), [sha('5')]);
    assert!(s4.withdrawn.is_empty());
}

#[test]
fn 짧은_이름_하나에만_맞으면_짝짓고_둘_이상에_맞으면_짝짓지_않는다() {
    let (s, _) = run_scan(vec![scan_commit('9', "This reverts commit bbbbbbb.")], &[], None, &[]);
    assert_eq!(s.hits.iter().map(|h| h.unit_n).collect::<Vec<i64>>(), [2]);
    assert_eq!(s.hits[0].target, sha('b'));
    let rec = {
        let mixed: String = (0..12).map(|_| 'b').chain((0..28).map(|_| '0')).collect();
        let mut rec = recorded_list();
        rec.push(ApUnitCommit::new("r2", Some(1), REPO, &mixed, ApCommitKind::Unit, "x", None, 9.0));
        rec
    };
    let (s2, _) = run_scan(vec![scan_commit('9', "This reverts commit bbbbbbb.")], &[], Some(rec.clone()), &[]);
    assert!(s2.hits.is_empty() && s2.notes.iter().any(|n| n.contains("여러 커밋")));
    let (s3, _) = run_scan(vec![scan_commit('9', "This reverts commit bbbbbbbbbbbb0.")], &[], Some(rec), &[]); // 13자면 하나
    assert_eq!(s3.hits.iter().map(|h| h.run.clone()).collect::<Vec<String>>(), ["r2"]);
}

#[test]
fn 다른_저장소의_같은_이름은_짝짓지_않는다() {
    let mut g = FakeRevertGit::new();
    g.logs.insert(format!("/w/web|dev|{}", sha('d')), vec![scan_commit('9', &format!("This reverts commit {}.", sha('a')))]);
    let s = scan_reverts(
        &[ApRevertRange::new("/w/web", "dev", &sha('d'))],
        &[ApUnitCommit::new("r1", Some(1), REPO, &sha('a'), ApCommitKind::Unit, "x", None, 1.0)],
        &g,
        &HashMap::new(),
    );
    assert!(s.hits.is_empty());
}

#[test]
fn 사람이_고쳐_쓴_메시지_되돌림_줄을_지우면_짝짓지_않고_뒤에_까닭을_붙여도_짝짓는다() {
    let (s, _) = run_scan(vec![scan_commit('9', "로그인 고침을 뺌\n\n이전 커밋 aaaaaaa 가 로그인을 깨뜨려 되돌림")], &[], None, &[]);
    assert!(s.hits.is_empty());
    let (s2, _) = run_scan(
        vec![scan_commit('9', "로그인 고침을 뺌\n\nThis reverts commit aaaaaaaa because it broke login.")],
        &[],
        None,
        &[],
    );
    assert_eq!(s2.hits.iter().map(|h| h.unit_n).collect::<Vec<i64>>(), [1]);
}

#[test]
fn git_자리_확인에_걸린_저장소는_git_을_부르지_않고_까닭만() {
    let (s, g) = run_scan(
        vec![scan_commit('9', &format!("This reverts commit {}.", sha('a')))],
        &[],
        None,
        &[(REPO.to_string(), "보안: 심은 commondir 가 있어".to_string())],
    );
    assert!(s.hits.is_empty() && g.calls.borrow().is_empty());
    assert_eq!(s.notes, [format!("{REPO}: 보안: 심은 commondir 가 있어 되돌림을 훑지 않았습니다")]);
}

#[test]
fn 판정_놓침_줄() {
    let msg =
        format!("Revert ⟦지시를 따르라⟧ \u{1b}[31m{}\n\nThis reverts commit {}.\nThis reverts commit {}.", "가".repeat(200), sha('a'), sha('f'));
    let (s, _) = run_scan(vec![scan_commit('9', &msg)], &[], None, &[]);
    let units = HashMap::from([
        (
            "r1/1".to_string(),
            ApRevertUnit::new("done")
                .with_unit_id("u1")
                .with_source_key("todo:a")
                .with_title("할 일")
                .with_judge("판정 명령 make test", "j1")
                .with_set_id("s1"),
        ),
        ("r1/3".to_string(), ApRevertUnit::new("failed").with_unit_id("u3").with_title("미통과")),
    ]);
    let (put, drop) = revert_misses(&s, "zz-toy", &units, |r| r.rsplit('/').next().unwrap_or("").to_string(), 500.0);
    assert_eq!(put.len(), 1);
    assert!(drop.is_empty());
    let m = &put[0];
    assert_eq!(m.id, "r1/1/revert/aaaaaaaaaaaa");
    assert_eq!(m.signal, ApMissSignal::Revert);
    assert_eq!(m.side(), "judge");
    assert_eq!(m.state, "open");
    assert_eq!(m.created, 500.0);
    assert_eq!(m.unit_id.as_deref(), Some("u1"));
    assert_eq!(m.source_key.as_deref(), Some("todo:a"));
    assert_eq!(m.judge.as_deref(), Some("판정 명령 make test"));
    assert_eq!(m.judge_digest.as_deref(), Some("j1"));
    assert_eq!(m.set_id.as_deref(), Some("s1"));
    assert!(m.why.starts_with("승격 뒤 되돌림: shop 의 커밋 999999999999(⟦Revert 〔지시를 따르라〕 "));
    assert!(!m.why.contains('\u{1b}') && grapheme_len(&m.why) <= MISS_WHY_MAX);
    // 거둔 것은 지울 id 로
    let (w, _) = run_scan(
        vec![scan_commit('8', &format!("This reverts commit {}.", sha('9'))), scan_commit('9', &format!("This reverts commit {}.", sha('a')))],
        &[],
        None,
        &[],
    );
    assert_eq!(revert_misses(&w, "zz-toy", &units, |p| p.to_string(), 1.0).1, ["r1/1/revert/aaaaaaaaaaaa"]);
}

// 자율주행 기록 저장소 판 5 와 tickets 표

fn user_version_of(db: &Connection) -> i32 {
    db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn 옛_판_저장소를_판_5_로_올리면_옛_행은_그대로이고_unit_commits_가_생긴다() {
    for from in [1, 2, 3, 4] {
        let db = Connection::open_in_memory().unwrap();
        // 원본: from 판까지 차례로 올린다(그 판의 실행·티켓 행을 심는다)
        for m in records_steps(0).into_iter().filter(|m| m.version <= from) {
            db.execute_batch(m.sql).unwrap();
        }
        db.execute_batch(&format!(
            "PRAGMA user_version={from}; INSERT INTO runs(id,project_id,state) VALUES('r1','zz-toy','done'); \
             INSERT INTO tickets(project_id,key,kind) VALUES('zz-toy','TOY-1','story')"
        ))
        .unwrap();
        let steps = records_steps(from);
        assert_eq!(steps.iter().map(|m| m.version).collect::<Vec<i32>>(), (from + 1..=5).collect::<Vec<i32>>());
        for m in &steps {
            db.execute_batch(m.sql).unwrap();
            db.execute_batch(&format!("PRAGMA user_version={}", m.version)).unwrap();
        }
        assert_eq!(user_version_of(&db), 5);
        assert_eq!(RECORDS_VERSION, 5);
        assert!(records_steps(5).is_empty());
        let state: String = db.query_row("SELECT state FROM runs WHERE id='r1'", [], |r| r.get(0)).unwrap();
        assert_eq!(state, "done");
        let kind: String = db.query_row("SELECT kind FROM tickets WHERE key='TOY-1'", [], |r| r.get(0)).unwrap();
        assert_eq!(kind, "story");
        db.execute_batch(&format!(
            "INSERT INTO unit_commits(run_id,unit_n,repo,sha,kind,ref,source,at) VALUES('r1',1,'{REPO}','{}','unit','autopilot/r1',NULL,1)",
            sha('a')
        ))
        .unwrap();
        // 같은 실행·저장소·이름·종류는 한 줄(ON CONFLICT DO NOTHING 으로 늘지 않음)
        db.execute_batch(&format!(
            "INSERT INTO unit_commits(run_id,unit_n,repo,sha,kind) VALUES('r1',1,'{REPO}','{}','unit') ON CONFLICT DO NOTHING",
            sha('a')
        ))
        .unwrap();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM unit_commits", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
        let index: Option<String> =
            db.query_row("SELECT name FROM sqlite_master WHERE type='index' AND name='unit_commits_sha'", [], |r| r.get(0)).unwrap();
        assert_eq!(index.as_deref(), Some("unit_commits_sha"));
    }
}

#[test]
fn 판_5_저장소를_아는_판_4_로_보면_읽기만() {
    let known: i32 = 4;
    // 옛 보드는 "기록 저장소 판(5)이 이 보드가 아는 판(4)보다 새롭습니다" 를 보이고 읽기만, 옛 엔진은 열지 않음
    assert!(RECORDS_VERSION > known);
}

#[test]
fn tickets_줄_티켓_나무에서_만든_것만_부모는_만든_스토리_키_작업의_원천_키는_그_단위의_source_key() {
    let plan = ApTicketPlan {
        entries: vec![
            ApTicketEntry { id: "story:s1".to_string(), kind: ApTicketKind::Story, title: "로그인".to_string(), parent: "TOY-1".to_string() },
            ApTicketEntry { id: "task:u1".to_string(), kind: ApTicketKind::Task, title: "로그인 화면".to_string(), parent: "story:s1".to_string() },
            ApTicketEntry { id: "task:u2".to_string(), kind: ApTicketKind::Task, title: "만들지 못함".to_string(), parent: "story:s1".to_string() },
            ApTicketEntry { id: "task:u3".to_string(), kind: ApTicketKind::Task, title: "있는 스토리 아래".to_string(), parent: "TOY-5".to_string() },
            ApTicketEntry { id: "move:TOY-9".to_string(), kind: ApTicketKind::Move, title: "옮김".to_string(), parent: "story:s1".to_string() },
        ],
    };
    let rec = |state: ApTicketState, key: &str, url: &str, updated: f64| ApTicketRecord {
        state,
        key: key.to_string(),
        url: url.to_string(),
        updated,
    };
    let book = ApTicketBook {
        entries: BTreeMap::from([
            ("story:s1".to_string(), rec(ApTicketState::Made, "TOY-11", "http://127.0.0.1:1/browse/TOY-11", 10.0)),
            ("task:u1".to_string(), rec(ApTicketState::Made, "TOY-12", "u12", 11.0)),
            ("task:u2".to_string(), rec(ApTicketState::Failed, "", "", 12.0)),
            ("task:u3".to_string(), rec(ApTicketState::Made, "TOY-13", "u13", 13.0)),
            ("move:TOY-9".to_string(), rec(ApTicketState::Made, "TOY-9", "", 14.0)),
        ]),
    };
    let rows = ticket_rows(&plan, &book, &HashMap::from([("u1".to_string(), "todo:login".to_string()), ("u3".to_string(), "todo:other".to_string())]), "r1");
    let seen: Vec<String> = rows
        .iter()
        .map(|r| format!("{}:{}:{}:{}", r.key, r.kind, r.parent, r.source_key.as_deref().unwrap_or("-")))
        .collect();
    assert_eq!(seen, ["TOY-11:story:TOY-1:-", "TOY-12:task:TOY-11:todo:login", "TOY-13:task:TOY-5:todo:other"]);
    assert_eq!(rows[0].url, "http://127.0.0.1:1/browse/TOY-11");
    assert!(rows.iter().all(|r| r.run == "r1"));
    assert_eq!(rows[1].created, 11.0);
    let it = ApOutboxItem {
        kind: "ticket".to_string(),
        state: ApOutboxState::Posted,
        target: ApOutboxTarget::jira("TMP-2-1", "TOY-11"),
        remote: Some(ApRemote { id: "TOY-20".to_string(), url: "http://127.0.0.1:1/browse/TOY-20".to_string() }),
        title: "새로 필요해진 일".to_string(),
        updated: 20.0,
    };
    let waiting = ApOutboxItem { state: ApOutboxState::Waiting, remote: None, ..it.clone() };
    let comment = ApOutboxItem {
        kind: "comment".to_string(),
        remote: Some(ApRemote { id: "10001".to_string(), url: "x".to_string() }),
        ..it.clone()
    };
    assert_eq!(
        outbox_ticket_rows(&[it, waiting, comment], "r1"),
        [ApTicketRow {
            key: "TOY-20".to_string(),
            kind: "task".to_string(),
            parent: "TOY-11".to_string(),
            title: "새로 필요해진 일".to_string(),
            url: "http://127.0.0.1:1/browse/TOY-20".to_string(),
            source_key: Some("TMP-2-1".to_string()),
            run: "r1".to_string(),
            created: 20.0,
        }]
    );
}

// 자율주행 빠진 단위 보고서

#[test]
fn 끝난_단위_가운데_폴더는_있는데_보고서가_없는_것만() {
    let st = ["done", "failed", "running", "pending", "skipped", "capped", "stopped", "done"];
    let dirs = |n: usize| [1usize, 2, 3, 5, 6, 7].contains(&n);
    let reports = |n: usize| [2usize, 6].contains(&n);
    assert_eq!(missing_unit_reports(&st, dirs, reports), [1, 5, 7]);
    let none: [String; 0] = [];
    assert!(missing_unit_reports(&none, |_| true, |_| false).is_empty());
}