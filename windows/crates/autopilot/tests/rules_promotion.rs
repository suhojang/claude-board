//! 가지 자리·승격 차례 규칙(`rules::promotion`) 시험.
//! 원본: `Tests/AutopilotRulesTests/PromotionTests.swift` 의 `PromotionFillTests`(표 11줄을 도는 시험 `표` 1개)와
//! `Tests/AutopilotRulesTests/StartTests.swift` 의 `PromotionEditTests`(표 10줄을 도는 시험 `표` 1개 + `고를것`). Swift `@Test` 3개가 Rust 시험
//! `fill_표`·`edit_표`·`고를것` 으로 옮겨졌다. 표의 줄은 Swift 에서 기계로 옮겼고(손으로 다시 치지 않았다: `"…"` → `r#"…"#`, 나머지 글자 그대로)
//! 표 길이를 세어 줄이 빠지면 실패한다. 끝의 `extra_` 시험은 Swift 시험에 없는 직접 호출·경계·Windows 가지 이름 꼴이다.

use claude_board_autopilot::rules::branches::Step;
use claude_board_autopilot::rules::promotion::{branch_roles, default_promotion, promotion_choices, promotion_problems};

fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

/// Swift `steps(_:)` — "autopilot>dev autopilot>live@dev" 를 단계로
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

fn show(steps: &[Step]) -> String {
    steps.iter().map(|s| format!("{}>{}{}", s.from, s.into, s.after.as_ref().map(|a| format!("@{a}")).unwrap_or_default())).collect::<Vec<_>>().join(" ")
}

// ── 표 (Swift RolesRow) ───────────────────────────────

struct RolesRow {
    #[allow(dead_code)]
    note: &'static str,
    branches: &'static [&'static str],
    want: &'static str,
    steps: &'static str,
}

const ROLES_ROWS: &[RolesRow] = &[
    RolesRow { note: r#"가지가 없음 → 자리도 단계도 없음"#, branches: &[], want: r#"-/-/-"#, steps: r#""# },
    RolesRow { note: r#"dev·live·master → 세 단계(장난감 zz-ap53)"#, branches: &[r#"autopilot/zz-ap53-jira"#, r#"dev"#, r#"live"#, r#"main"#, r#"master"#], want: r#"dev/live/master"#, steps: r#"autopilot>dev autopilot>live@dev live>master@live"# },
    RolesRow { note: r#"master 가 없으면 main 이 그 자리(장난감 zz-ap52)"#, branches: &[r#"autopilot/zz-ap52-p1"#, r#"dev"#, r#"live"#, r#"main"#], want: r#"dev/live/main"#, steps: r#"autopilot>dev autopilot>live@dev live>main@live"# },
    RolesRow { note: r#"main 하나 → 자율주행 가지 → main 한 단계"#, branches: &[r#"main"#], want: r#"-/-/main"#, steps: r#"autopilot>main"# },
    RolesRow { note: r#"develop·staging·main 은 같은 자리로 짝짓는다"#, branches: &[r#"develop"#, r#"staging"#, r#"main"#], want: r#"develop/staging/main"#, steps: r#"autopilot>develop autopilot>staging@develop staging>main@staging"# },
    RolesRow { note: r#"dev 와 develop 이 함께 있으면 dev"#, branches: &[r#"develop"#, r#"dev"#, r#"main"#], want: r#"dev/-/main"#, steps: r#"autopilot>dev autopilot>main@dev"# },
    RolesRow { note: r#"live 와 staging 이 함께 있으면 live"#, branches: &[r#"staging"#, r#"live"#], want: r#"-/live/-"#, steps: r#"autopilot>live"# },
    RolesRow { note: r#"master 와 main 이 함께 있으면 master"#, branches: &[r#"main"#, r#"master"#], want: r#"-/-/master"#, steps: r#"autopilot>master"# },
    RolesRow { note: r#"live 없이 dev·master → master ← 자율주행 가지(dev 뒤)"#, branches: &[r#"dev"#, r#"master"#], want: r#"dev/-/master"#, steps: r#"autopilot>dev autopilot>master@dev"# },
    RolesRow { note: r#"dev 없이 live·master → live 는 뒤 조건 없음"#, branches: &[r#"live"#, r#"master"#], want: r#"-/live/master"#, steps: r#"autopilot>live live>master@live"# },
    RolesRow { note: r#"다른 이름 가지는 자리가 아니다"#, branches: &[r#"feature/x"#, r#"autopilot/zz"#], want: r#"-/-/-"#, steps: r#""# },
];

#[test]
fn fill_표() {
    assert_eq!(ROLES_ROWS.len(), 11, "Swift rolesRows 는 11줄");
    for r in ROLES_ROWS {
        let b = v(r.branches);
        let n = branch_roles(&b);
        let got = format!("{}/{}/{}", n.dev.as_deref().unwrap_or("-"), n.live.as_deref().unwrap_or("-"), n.master.as_deref().unwrap_or("-"));
        assert_eq!(got, r.want, "{}", r.note);
        assert_eq!(show(&default_promotion(&b)), r.steps, "{}", r.note);
    }
}

// ── 표 (Swift PromoRow) ───────────────────────────────

struct PromoRow {
    #[allow(dead_code)]
    note: &'static str,
    steps: &'static str,
    branches: &'static [&'static str],
    want: &'static [&'static str],
}

const PROMO_ROWS: &[PromoRow] = &[
    PromoRow { note: r#"저장소 가지로 채운 기본 차례는 고칠 것이 없다"#, steps: r#"autopilot>dev autopilot>live@dev live>master@live"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[] },
    PromoRow { note: r#"main 하나"#, steps: r#"autopilot>main"#, branches: &[r#"main"#], want: &[] },
    PromoRow { note: r#"없는 가지로 받기"#, steps: r#"autopilot>develop"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 1: 받는 가지 develop 가 저장소에 없습니다"#] },
    PromoRow { note: r#"앞 단계가 받지 않은 가지를 올리기"#, steps: r#"autopilot>dev live>master@dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 2: live 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)"#] },
    PromoRow { note: r#"뒤 조건이 앞에 없는 가지"#, steps: r#"autopilot>live@dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 1: dev 뒤라는 조건의 단계가 앞에 없습니다"#] },
    PromoRow { note: r#"뒤 조건이 받는 가지 자신"#, steps: r#"autopilot>dev@dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 1: 뒤 조건이 받는 가지 자신(dev)입니다"#] },
    PromoRow { note: r#"같은 단계 두 번"#, steps: r#"autopilot>dev autopilot>dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 2: 같은 단계(자율주행 가지 → dev)가 두 번 있습니다"#] },
    PromoRow { note: r#"같은 가지끼리"#, steps: r#"autopilot>dev dev>dev@dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[r#"단계 2: 같은 가지(dev)끼리는 올리지 않습니다"#, r#"단계 2: 뒤 조건이 받는 가지 자신(dev)입니다"#] },
    PromoRow { note: r#"live 를 빼고 master 로 바로(dev 뒤)"#, steps: r#"autopilot>dev autopilot>master@dev"#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[] },
    PromoRow { note: r#"빈 차례는 고칠 것이 아니다(엔진은 차례 없이 합치기만 하고 기존 가지끼리는 거부)"#, steps: r#""#, branches: &[r#"dev"#, r#"live"#, r#"master"#], want: &[] },
];

#[test]
fn edit_표() {
    assert_eq!(PROMO_ROWS.len(), 10, "Swift promoRows 는 10줄");
    for r in PROMO_ROWS {
        let got = promotion_problems(&steps(r.steps), &v(r.branches));
        assert_eq!(got, v(r.want), "{}", r.note);
    }
}

#[test]
fn 고를것() {
    let b = v(&["dev", "live", "master"]);
    let s = steps("autopilot>dev autopilot>live@dev live>master@live");
    let c0 = promotion_choices(&s, 0, &b);
    assert!(c0.from == v(&["autopilot"]) && c0.after.is_empty() && c0.into == v(&["dev", "live", "master"]));
    let c2 = promotion_choices(&s, 2, &b);
    assert!(c2.from == v(&["autopilot", "dev", "live"]) && c2.after == v(&["dev", "live"]) && c2.into == v(&["dev", "master"]));
    // 새 단계(끝 다음)
    assert_eq!(promotion_choices(&s, 3, &b).after, v(&["dev", "live", "master"]));
}

// ── 추가: Swift 시험에 없는 것 ───────────────────────────

#[test]
fn extra_steps_helper_matches_swift_split() {
    assert_eq!(steps("autopilot>dev autopilot>live@dev"), vec![Step::new("autopilot", "dev", None), Step::new("autopilot", "live", Some("dev"))]);
    assert!(steps("").is_empty());
}

#[test]
fn extra_choices_index_past_the_end_and_duplicates() {
    let b = v(&["dev", "live"]);
    // 받는 가지가 겹치는 앞 단계는 한 번만(순서 유지)
    let s = steps("autopilot>dev autopilot>dev autopilot>live@dev");
    let c = promotion_choices(&s, 2, &b);
    assert_eq!(c.from, v(&["autopilot", "dev"]));
    assert_eq!(c.after, v(&["dev"]));
    // 끝 훨씬 너머(Swift 는 min 으로 끝에 맞춘다) — 올릴 가지는 자율주행 가지
    let far = promotion_choices(&s, 99, &b);
    assert_eq!(far.from, v(&["autopilot", "dev", "live"]));
    assert_eq!(far.into, v(&["dev", "live"]));
    // 올릴 가지가 가지 이름이면 받는 가지 목록에서 빠진다
    let s2 = steps("autopilot>dev dev>live@dev");
    assert_eq!(promotion_choices(&s2, 1, &b).into, v(&["live"]));
    // 새 단계의 올릴 가지는 자율주행 가지라서 저장소에 같은 이름의 가지가 있으면 받는 가지에서 빠진다(Swift `filter { $0 != from }`)
    assert_eq!(promotion_choices(&s, 3, &v(&["autopilot", "dev"])).into, v(&["dev"]));
    // 빈 차례
    let e =promotion_choices(&[], 0, &b);
    assert!(e.from == v(&["autopilot"]) && e.after.is_empty() && e.into == b);
}

#[test]
fn extra_problems_collects_in_order_and_empty_input() {
    assert!(promotion_problems(&[], &[]).is_empty());
    // 한 단계에서 여러 문제: 없는 받는 가지 + 같은 가지 + 뒤 조건 자신
    let got = promotion_problems(&steps("x>x@x"), &v(&["dev"]));
    assert_eq!(got, v(&["단계 1: 받는 가지 x 가 저장소에 없습니다", "단계 1: 같은 가지(x)끼리는 올리지 않습니다", "단계 1: 뒤 조건이 받는 가지 자신(x)입니다"]));
    // 올릴 가지가 저장소에 없음
    let got = promotion_problems(&steps("autopilot>dev ghost>dev"), &v(&["dev"]));
    assert_eq!(got, v(&["단계 2: 올릴 가지 ghost 가 저장소에 없습니다"]));
}

#[test]
fn extra_problems_duplicate_step_names_the_from_branch() {
    let got = promotion_problems(&steps("autopilot>dev dev>live@dev dev>live@dev"), &v(&["dev", "live"]));
    assert_eq!(got, v(&["단계 3: 같은 단계(dev → live)가 두 번 있습니다"]));
}

#[test]
fn extra_default_promotion_with_extra_unrelated_branches_and_order() {
    let b = v(&["zzz", "main", "autopilot/x", "develop", "staging"]);
    let n = branch_roles(&b);
    assert_eq!((n.dev.as_deref(), n.live.as_deref(), n.master.as_deref()), (Some("develop"), Some("staging"), Some("main")));
    assert_eq!(show(&default_promotion(&b)), "autopilot>develop autopilot>staging@develop staging>main@staging");
    // 기본 차례는 언제나 고칠 것이 없다
    assert!(promotion_problems(&default_promotion(&b), &b).is_empty());
}

// ── Windows: 가지 이름은 글 그대로 견준다(대소문자·공백·`\`·정규화) ──

#[test]
fn extra_windows_branch_names_are_compared_verbatim_text() {
    // 대소문자가 다르면 다른 이름: 자리가 아니다
    let n = branch_roles(&v(&["Dev", "LIVE", "Main"]));
    assert_eq!((n.dev, n.live, n.master), (None, None, None));
    assert!(default_promotion(&v(&["Dev", "Main"])).is_empty());
    // 끝 공백·`\`·`/` 꼴도 다른 이름
    assert_eq!(branch_roles(&v(&["dev ", "main\\"])), Default::default());
    assert_eq!(branch_roles(&v(&["origin/dev", "feature\\dev"])), Default::default());
    // 받는 가지: 대소문자가 다르면 저장소에 없음(더 엄격한 쪽, Windows NTFS 의 대소문자 무시로 느슨해지지 않는다)
    let got = promotion_problems(&steps("autopilot>dev"), &v(&["Dev"]));
    assert_eq!(got, v(&["단계 1: 받는 가지 dev 가 저장소에 없습니다"]));
    let got = promotion_problems(&steps("autopilot>main"), &v(&["main "]));
    assert_eq!(got.len(), 1);
    // 올릴 가지·뒤 조건도 같다: `Dev` 를 받은 단계 뒤에 `dev` 를 올릴 수 없다
    let got = promotion_problems(&steps("autopilot>Dev dev>live"), &v(&["Dev", "dev", "live"]));
    assert_eq!(got, v(&["단계 2: dev 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)"]));
    let got = promotion_problems(&steps("autopilot>Dev autopilot>live@dev"), &v(&["Dev", "live"]));
    assert_eq!(got, v(&["단계 2: dev 뒤라는 조건의 단계가 앞에 없습니다"]));
    // 단계 중복 키도 글 그대로: 대소문자만 다른 두 단계는 중복이 아니다
    assert!(promotion_problems(&steps("autopilot>dev autopilot>Dev"), &v(&["dev", "Dev"])).is_empty());
    // "autopilot" 은 소문자 그대로만 자율주행 가지: `Autopilot` 은 기존 가지로 취급돼 저장소에 있어야 한다
    let got = promotion_problems(&steps("Autopilot>dev"), &v(&["dev"]));
    assert_eq!(got, v(&["단계 1: 올릴 가지 Autopilot 가 저장소에 없습니다"]));
}

#[test]
fn extra_windows_unicode_names_are_not_normalized() {
    // NFC "é"(U+00E9) 와 NFD "e"+U+0301 은 Swift 에서는 같은 문자열이고 Rust 에서는 다른 이름이다(알려진 차이)
    let nfc = "dev-\u{e9}";
    let nfd = "dev-e\u{301}";
    let got = promotion_problems(&steps(&format!("autopilot>{nfc}")), &v(&[nfd]));
    assert_eq!(got.len(), 1);
    assert!(promotion_problems(&steps(&format!("autopilot>{nfc}")), &v(&[nfc])).is_empty());
}
