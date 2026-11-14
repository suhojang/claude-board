//! 보고서 창 콘텐츠 규칙 준비 차례(`rules::report_gate`) 시험.
//! 원본: `Tests/AutopilotRulesTests/ReportGateTests.swift`(표 11 줄 × 1 시험 + 까닭 글 시험 1). Swift 표의 줄을 같은 입력·같은 기대값으로 옮겼다.
//! 줄마다 사건(창이 싣고 싶음 want·준비 끝 ok/fail·창 닫힘)을 차례로 넣고, 앱이 할 일을 글자로 이어 견준다.
//! 글자 꼴: compile · wait:<창> · load:<창> · refuse:<창>. 표 길이를 세어 줄이 빠지면 실패한다.

use claude_board_autopilot::rules::report_gate::{rule_gate_why, RuleGate, RuleGateAction, RuleGatePhase, RULE_GATE_WAIT_TEXT};

struct GateRow {
    note: &'static str,
    events: &'static [&'static str], // "want:A" · "ok" · "fail" · "close:A"
    actions: &'static str,           // 사건마다 할 일을 " | " 로, 한 사건 안은 "," 로
    phase: RuleGatePhase,
    tries: u32,
}

fn act(a: &RuleGateAction) -> String {
    match a {
        RuleGateAction::Compile => "compile".to_string(),
        RuleGateAction::Wait(w) => format!("wait:{w}"),
        RuleGateAction::Load(w) => format!("load:{w}"),
        RuleGateAction::Refuse(w, _) => format!("refuse:{w}"),
    }
}

fn joined(a: &[RuleGateAction]) -> String {
    a.iter().map(act).collect::<Vec<_>>().join(",")
}

fn row(note: &'static str, events: &'static [&'static str], actions: &'static str, phase: RuleGatePhase, tries: u32) -> GateRow {
    GateRow { note, events, actions, phase, tries }
}

fn gate_rows() -> Vec<GateRow> {
    use RuleGatePhase::{Compiling, Ready, Failed};
    vec![
        row("첫 창: 컴파일하고 기다린 뒤 싣는다", &["want:A", "ok"], "compile,wait:A | load:A", Ready, 1),
        row(
            "컴파일이 끝나기 전에 연 둘째 창도 기다렸다가 함께 싣는다(전에는 규칙 없이 바로 load:B)",
            &["want:A", "want:B", "ok"],
            "compile,wait:A | wait:B | load:A,load:B",
            Ready,
            1,
        ),
        row("준비된 뒤 연 창은 바로 싣는다(다시 컴파일하지 않음)", &["want:A", "ok", "want:B"], "compile,wait:A | load:A | load:B", Ready, 1),
        row("같은 창을 기다리는 동안 다시 눌러도 한 번만 싣는다", &["want:A", "want:A", "ok"], "compile,wait:A | wait:A | load:A", Ready, 1),
        row("실패하면 기다린 창 모두 싣지 않고 까닭(닫힌 쪽)", &["want:A", "want:B", "fail"], "compile,wait:A | wait:B | refuse:A,refuse:B", Failed, 1),
        row(
            "실패 뒤 다음 창을 열면 다시 컴파일한다(전에는 다시 하지 않고 규칙 없이 load)",
            &["want:A", "fail", "want:B", "ok"],
            "compile,wait:A | refuse:A | compile,wait:B | load:B",
            Ready,
            2,
        ),
        row("실패한 창의 다시 해 보기도 같은 길", &["want:A", "fail", "want:A", "ok"], "compile,wait:A | refuse:A | compile,wait:A | load:A", Ready, 2),
        row(
            "두 번 실패해도 셋째 요청이 또 컴파일",
            &["want:A", "fail", "want:A", "fail", "want:A"],
            "compile,wait:A | refuse:A | compile,wait:A | refuse:A | compile,wait:A",
            Compiling,
            3,
        ),
        row("기다리던 창이 닫히면 컴파일이 끝나도 싣지 않는다", &["want:A", "want:B", "close:A", "ok"], "compile,wait:A | wait:B | - | load:B", Ready, 1),
        row("컴파일 중이 아닐 때 온 끝(늦은 콜백)은 무시", &["want:A", "ok", "fail"], "compile,wait:A | load:A | -", Ready, 1),
        row("아무도 기다리지 않을 때 끝나도 준비됨으로", &["want:A", "close:A", "ok", "want:B"], "compile,wait:A | - | - | load:B", Ready, 1),
    ]
}

#[test]
fn gate_rows_table_has_all_rows() {
    // Swift 표는 11 줄이다. 줄이 빠지면 여기서 실패한다
    assert_eq!(gate_rows().len(), 11);
}

#[test]
fn 차례() {
    for r in gate_rows() {
        let mut g = RuleGate::new();
        let mut out: Vec<String> = Vec::new();
        for e in r.events {
            if let Some(w) = e.strip_prefix("want:") {
                out.push(joined(&g.want(w)));
            } else if let Some(w) = e.strip_prefix("close:") {
                g.closed(w);
                out.push("-".to_string());
            } else {
                let ok = *e == "ok";
                let a = joined(&g.compiled(ok, if ok { None } else { Some("toy compile error") }));
                out.push(if a.is_empty() { "-".to_string() } else { a });
            }
        }
        assert_eq!(out.join(" | "), r.actions, "{}", r.note);
        assert_eq!(g.phase(), r.phase, "{}", r.note);
        assert_eq!(g.tries(), r.tries, "{}", r.note);
        assert!(g.waiting().is_empty() || r.phase == RuleGatePhase::Compiling, "{}", r.note);
    }
}

#[test]
fn 까닭() {
    let mut g = RuleGate::new();
    let _ = g.want("A");
    let a = g.compiled(false, Some("  Rule list compilation failed  "));
    assert_eq!(
        a,
        vec![RuleGateAction::Refuse(
            "A".to_string(),
            "바깥 요청 막기 규칙을 만들지 못해 보고서를 싣지 않았습니다(Rule list compilation failed). 다시 해 보기를 누르거나 창을 다시 열면 규칙을 다시 만듭니다.".to_string()
        )]
    );
    assert!(g.last_error().unwrap().starts_with("바깥 요청 막기 규칙을"));
    assert_eq!(
        rule_gate_why(None),
        "바깥 요청 막기 규칙을 만들지 못해 보고서를 싣지 않았습니다. 다시 해 보기를 누르거나 창을 다시 열면 규칙을 다시 만듭니다."
    );
    assert!(rule_gate_why(Some(&"x".repeat(300))).contains(&format!("{}…", "x".repeat(160))));
    let _ = g.want("A");
    let _ = g.compiled(true, None);
    assert_eq!(g.last_error(), None, "다시 된 뒤에는 까닭을 지운다");
}

// ── 추가: Swift 시험에 없는 경계 ─────────────────────

#[test]
fn 추가_까닭_경계와_기다림_글() {
    // 160 글자는 그대로, 161 글자부터 자르고 …
    let exact = "y".repeat(160);
    assert!(rule_gate_why(Some(&exact)).contains(&format!("({exact}).")));
    let over = "y".repeat(161);
    assert!(rule_gate_why(Some(&over)).contains(&format!("({}…).", "y".repeat(160))));
    // 공백뿐이면 비어 있는 것으로
    assert_eq!(rule_gate_why(Some(" \n\t ")), rule_gate_why(None));
    // 한글은 글자 단위로 센다
    let ko = "가".repeat(161);
    assert!(rule_gate_why(Some(&ko)).contains(&format!("({}…).", "가".repeat(160))));
    assert_eq!(RULE_GATE_WAIT_TEXT, "바깥 요청 막기 규칙을 준비하는 중입니다. 규칙이 붙은 뒤에 보고서를 싣습니다.");
}
