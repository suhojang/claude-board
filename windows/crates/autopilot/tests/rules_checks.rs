//! 판정 실패 글(`rules::checks`)과 근거 묶음(`rules::evidence`) 시험.
//! 원본: `Tests/AutopilotRulesTests/ChecksTests.swift`(사례 18), `JudgeMoveTests.swift`(출력 끝 53꼴 + 카드 3),
//! `ReadingTests.swift` 「거부」의 MANIFEST 절반(1). `add_evidence` 를 지나는 사례(Reading 「묶음」·「표시」·「거부」, Import 「묶음」)는
//! rules_reading.rs·rules_import.rs 에 있다.
use claude_board_autopilot::rules::checks::*;
use claude_board_autopilot::rules::evidence::*;
use serde_json::json;

struct CheckEventRow {
    note: &'static str,
    code: i32,
    tries: i64,
    retries: i64,
    timed_out: Option<f64>,
    want: &'static str,
}

fn event_rows() -> Vec<CheckEventRow> {
    vec![
        CheckEventRow { note: "첫 판정 실패 → 다시 맡김 1/2", code: 1, tries: 0, retries: 2, timed_out: None,
            want: "판정 실패(종료 코드 1). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2)." },
        CheckEventRow { note: "둘째 실패 → 2/2", code: 3, tries: 1, retries: 2, timed_out: None,
            want: "판정 실패(종료 코드 3). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(2/2)." },
        CheckEventRow { note: "횟수가 다 찼는데 다시 맡기면 사람의 말 때문", code: 1, tries: 2, retries: 2, timed_out: None,
            want: "판정 실패(종료 코드 1). 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다." },
        CheckEventRow { note: "시간 상한 없이 종료 코드 -1(띄우지 못함·보안으로 돌리지 않음)은 그대로 실패", code: -1, tries: 0, retries: 2, timed_out: None,
            want: "판정 실패(종료 코드 -1). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2)." },
        CheckEventRow { note: "시간 상한에 닿은 판정은 상한을 적고 실패로 봄", code: -1, tries: 0, retries: 2, timed_out: Some(6.0),
            want: "판정 명령이 시간 상한(6초)에 닿아 엔진이 멈췄습니다(실패로 봅니다). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2)." },
        CheckEventRow { note: "기본 상한 10분", code: -1, tries: 1, retries: 2, timed_out: Some(600.0),
            want: "판정 명령이 시간 상한(10분)에 닿아 엔진이 멈췄습니다(실패로 봅니다). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(2/2)." },
        CheckEventRow { note: "시간 상한 + 사람의 말로 한 번 더", code: -1, tries: 2, retries: 2, timed_out: Some(90.0),
            want: "판정 명령이 시간 상한(1분 30초)에 닿아 엔진이 멈췄습니다(실패로 봅니다). 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다." },
    ]
}

#[test]
fn 진행기록() {
    for r in event_rows() {
        assert_eq!(check_fail_event(r.code, r.tries, r.retries, r.timed_out), r.want, "{}", r.note);
    }
}

#[test]
fn 넘기는글() {
    let want = "판정 명령이 실패했습니다(종료 코드 2). 판정 명령: `test -f a.txt`\n\
                아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.\n\
                \n\
                ```\n\
                없음\n\
                ```";
    assert_eq!(check_fail_message(2, "test -f a.txt", "없음", None), want);
}

#[test]
fn 상한넘기는글() {
    let m = check_fail_message(-1, "./gradlew test", "(시간 상한 10분에 닿아 엔진이 멈췄습니다)", Some(600.0));
    assert!(m.starts_with("판정 명령이 시간 상한(10분)에 닿아 끝나지 않아 엔진이 멈췄습니다(실패로 봅니다). 판정 명령: `./gradlew test`\n"));
    assert!(m.contains("판정이 끝나지 않는 까닭"));
    assert!(m.contains("판정 명령과 시험 기준은 바꾸지 마세요"));
    assert!(!m.contains("종료 코드"));
    assert!(m.ends_with("```\n(시간 상한 10분에 닿아 엔진이 멈췄습니다)\n```"));
}

#[test]
fn 미통과() {
    assert_eq!(check_fail_reason(0, None), "판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.");
    assert_eq!(check_fail_reason(2, None), "2번 다시 해도 판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.");
    assert_eq!(
        check_fail_reason(2, Some(6.0)),
        "2번 다시 해도 판정 명령을 통과하지 못해(마지막 판정은 시간 상한 6초에 닿아 멈춤) 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다."
    );
}

#[test]
fn 상한시간() {
    for (sec, want) in [(6.0, "6초"), (0.4, "0초"), (59.6, "1분"), (60.0, "1분"), (600.0, "10분"), (90.0, "1분 30초"), (-3.0, "0초")] {
        assert_eq!(limit_text(sec), want, "sec={sec}");
    }
}

#[test]
fn 상한글() {
    assert_eq!(check_limit_note(Some(10.0)), " (10분 상한)");
    assert_eq!(check_limit_note(None), " (?분 상한)");
    assert_eq!(check_limit_note(Some(0.1)), " (6초 상한)");
    assert_eq!(check_limit_note(Some(2.5)), " (2분 30초 상한)");
}

// --- JudgeMoveTests: 옮기기 전 엔진의 식을 그대로 두고 규칙과 견준다 ---

fn old_tail(d: &[u8]) -> String {
    let start = d.len().saturating_sub(4000);
    let s = String::from_utf8_lossy(&d[start..]).into_owned();
    let lines: Vec<&str> = s.split('\n').collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

fn tail_inputs() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"\n".to_vec(),
        "한 줄".as_bytes().to_vec(),
        "끝 줄바꿈\n".as_bytes().to_vec(),
        "\r\n섞임\r\n".as_bytes().to_vec(),
    ];
    for lines in [1usize, 39, 40, 41, 120] {
        for width in [0usize, 3, 80, 400] {
            let line = "가".repeat(width / 3) + &"a".repeat(width % 3);
            let body = (0..lines).map(|i| format!("{i} {line}")).collect::<Vec<_>>().join("\n");
            out.push(body.clone().into_bytes());
            out.push((body + "\n").into_bytes());
        }
    }
    // 4000 바이트 경계가 한글(3 바이트) 가운데에 오게
    for shift in 0..8 {
        out.push(("x".repeat(shift) + &"판정".repeat(900)).into_bytes());
    }
    out
}

#[test]
fn 출력끝_옮기기_전_식과_같음() {
    let all = tail_inputs();
    assert_eq!(all.len(), 53);
    for d in &all {
        assert_eq!(output_tail(d), old_tail(d));
    }
}

#[test]
fn 출력끝_마지막_사십_줄과_사천_바이트() {
    let many = (0..100).map(|i| format!("줄{i}")).collect::<Vec<_>>().join("\n");
    let t = output_tail(many.as_bytes());
    assert_eq!(t.split('\n').count(), 40);
    assert!(t.starts_with("줄60\n") && t.ends_with("줄99"));
    // 4000 바이트 앞은 버린다
    let long = "a".repeat(10_000);
    assert_eq!(output_tail(long.as_bytes()).len(), 4000);
    // Windows 출력(CRLF)도 40줄로 센다, \r 은 줄 끝에 남는다
    let crlf = (0..60).map(|i| format!("l{i}\r\n")).collect::<String>();
    let t = output_tail(crlf.as_bytes());
    assert_eq!(t.split('\n').count(), 40);
    assert!(t.ends_with("l59\r\n"));
}

#[test]
fn 미리돌리기_카드() {
    let rows = [(1, "u1", "할 일 목록 API"), (3, "fix-12", "「따옴표」 와 \"글\""), (12, "u12", "")];
    for (n, id, title) in rows {
        let old = json!({
            "id": format!("prerun-{id}"), "unit": n, "kind": "prerun",
            "title": format!("「{title}」 의 판정이 일하기 전부터 통과합니다"),
            "why": "이미 끝난 일이거나 판정이 약하다는 뜻입니다. 그대로 두면 모델이 일하지 않아도 통과로 기록됩니다.",
            "options": [{"key": "drop", "label": "빼기"}, {"key": "fix_check", "label": "판정 고치기"}, {"key": "keep", "label": "그대로"}],
            "recommended": "drop",
        });
        assert_eq!(prerun_card(n, id, title), old);
    }
}

// --- 근거 묶음: 원본 시험은 apAddEvidence 를 거치는 것뿐이라 이름·MANIFEST 규칙을 직접 시험한다 ---

#[test]
fn 근거_파일_이름() {
    assert_eq!(evidence_name(1, "confluence", "48213"), "1-confluence-48213.json");
    // 묶음 시험 「묶음」의 Figma 이름(그림은 같은 이름의 .png)
    assert_eq!(evidence_name(2, "figma", "AbCdEfGhIjKlMn-12-4"), "2-figma-AbCdEfGhIjKlMn-12-4.json");
    assert_eq!(evidence_name(3, "mcp:wiki", ""), "3-mcp-wiki.json");
    assert_eq!(evidence_name(4, "", "x"), "4-web-x.json");
    assert_eq!(evidence_name(5, "jira", "a/b..c  d"), "5-jira-a-b-c-d.json");
    assert_eq!(evidence_name(6, "legacy", "zz-old/state/stop.md"), "6-legacy-zz-old-state-stop-md.json");
    assert_eq!(evidence_name(7, "jira", "한글-id"), "7-jira-id.json");
    assert_eq!(evidence_name(8, "jira", &"a".repeat(60)), format!("8-jira-{}.json", "a".repeat(40)));
}

fn sample(n: i64) -> EvidenceEntry {
    EvidenceEntry {
        n,
        file: format!("{n}-jira-A-1.json"),
        source: "jira".into(),
        via: "rest".into(),
        url: "https://example.atlassian.net/browse/A-1".into(),
        id: "A-1".into(),
        title: "제목".into(),
        read_at: 1_761_800_000.0,
        sha256: "sha-1".into(),
        bytes: 10,
        redacted: 2,
        requests: 1,
        ..Default::default()
    }
}

#[test]
fn manifest_왕복과_모양() {
    let mut a = sample(2);
    a.cost_usd = Some(0.01);
    a.image = Some(EvidenceImage { file: "2-jira-A-1.png".into(), host: "img.example".into(), sha256: "s2".into(), bytes: 5 });
    let entries = vec![a.clone(), sample(1)];
    let data = manifest_data(&entries, 1_761_800_060.0);
    let text = String::from_utf8(data.clone()).unwrap();
    assert!(text.find("\"entries\"").unwrap() < text.find("\"format\"").unwrap(), "키는 정렬");
    assert!(!text.contains("cost_usd") || text.matches("cost_usd").count() == 1, "비용은 있는 항목에만");
    let v = manifest_json(&entries, 1.0);
    assert_eq!(v["redacted"], 4);
    assert_eq!(v["entries"][0]["n"], 1, "항목은 번호 순");
    assert!(v["entries"][0].get("image").is_none() && v["entries"][0].get("cost_usd").is_none());
    let back = manifest_parse(Some(&data));
    assert_eq!(back, vec![sample(1), a]);
}

#[test]
fn manifest_묶음_밖_이름은_버린다() {
    let bad = br#"{"format":"claude-board-evidence","entries":[{"n":1,"file":"../../x.json"},{"n":2,"file":"2-ok.json","image":{"file":"/etc/x.png"}}]}"#;
    let p = manifest_parse(Some(bad));
    assert_eq!(p.iter().map(|e| e.file.as_str()).collect::<Vec<_>>(), vec!["2-ok.json"]);
    assert!(p[0].image.is_none());
    // Windows 구분자와 줄바꿈 꼬리도 거른다
    let win = br#"{"format":"claude-board-evidence","entries":[{"n":1,"file":"..\\x.json"},{"n":2,"file":"2-ok.json\n"},{"n":3,"file":"3-ok.json","image":{"file":"C:\\x.png"}}]}"#;
    let p = manifest_parse(Some(win));
    assert_eq!(p.iter().map(|e| e.n).collect::<Vec<_>>(), vec![3]);
    assert!(p[0].image.is_none());
}

#[test]
fn manifest_읽기_한계() {
    assert!(manifest_parse(None).is_empty());
    assert!(manifest_parse(Some(b"not json")).is_empty());
    assert!(manifest_parse(Some(br#"{"format":"other","entries":[]}"#)).is_empty());
    assert!(manifest_parse(Some(br#"{"format":"claude-board-evidence","entries":[1]}"#)).is_empty());
    // 번호 0·겹침은 버리고 앞의 것만 받는다, 번호 순 정렬
    let dup = r#"{"format":"claude-board-evidence","entries":[{"n":3,"file":"3-b.json","title":"앞"},{"n":0,"file":"0-a.json"},{"n":3,"file":"3-c.json","title":"뒤"},{"n":1,"file":"1-a.json"}]}"#.as_bytes();
    let p = manifest_parse(Some(dup));
    assert_eq!(p.iter().map(|e| (e.n, e.title.as_str())).collect::<Vec<_>>(), vec![(1, ""), (3, "앞")]);
    // 500개까지만
    let many = (1..=600).map(|n| format!(r#"{{"n":{n},"file":"{n}-a.json"}}"#)).collect::<Vec<_>>().join(",");
    let big = format!(r#"{{"format":"claude-board-evidence","entries":[{many}]}}"#);
    assert_eq!(manifest_parse(Some(big.as_bytes())).len(), 500);
    // 4 MiB 를 넘으면 읽지 않는다
    let huge = format!(r#"{{"format":"claude-board-evidence","entries":[],"pad":"{}"}}"#, "x".repeat(5 << 20));
    assert!(manifest_parse(Some(huge.as_bytes())).is_empty());
}
