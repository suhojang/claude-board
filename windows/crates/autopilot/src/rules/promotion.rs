//! 가지 자리(dev·live·master)와 승격 차례(`projects/<프로젝트>.json` promotion) 규칙. 원본: `Sources/AutopilotRules/Promotion.swift`.
//! git 은 부르지 않는다. 어댑터가 읽은 저장소 가지 이름 목록(`&[String]`)을 값으로 받는다. 순수 규칙이라 입출력이 없다.
//!
//! 어댑터 몫(git 입출력): 저장소 가지 이름 읽기(`git for-each-ref refs/heads`, 묶음이면 모든 저장소에 있는 가지의 교집합),
//! 승격 차례(`promotion`) 읽기·쓰기, 실제 합치기(merge)·가지 올리기. 여기서는 어느 것도 하지 않는다.
//!
//! Windows: 가지 이름은 글 그대로 견준다(대소문자·끝 공백·`\`·유니코드 정규화 모두 다른 이름). 그래서 `Dev` 는 `dev` 자리가 아니고
//! 저장소에 `Dev` 만 있으면 받는 가지 `dev` 는 "저장소에 없다"로 잡힌다(안전한 쪽). Swift 의 `String ==` 는 유니코드 정규 동등
//! (NFC·NFD 같음)이라 Rust 바이트 비교보다 느슨하다 — 같은 가지의 NFC·NFD 두 꼴이 섞여 들어오면 Rust 는 다른 이름으로 본다(어댑터는 git 이 준 글자를 그대로 넘긴다).

use crate::rules::branches::Step;

/// 가지 자리 셋. 없는 자리는 `None`. 원본: `apBranchRoles` 의 반환 튜플
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BranchRoles {
    pub dev: Option<String>,
    pub live: Option<String>,
    pub master: Option<String>,
}

/// 바꾸기 화면의 고를 수 있는 것. 원본: `apPromotionChoices` 의 반환 튜플
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionChoices {
    /// 받는 가지
    pub into: Vec<String>,
    /// 올릴 것
    pub from: Vec<String>,
    /// 뒤 조건
    pub after: Vec<String>,
}

fn first_of(names: &[&str], b: &[String]) -> Option<String> {
    names.iter().find(|n| b.iter().any(|x| x == **n)).map(|n| (*n).to_string())
}

/// 가지 자리 — dev(develop), live(staging), master(main). 앞 이름이 먼저다(둘 다 있으면 dev·live·master).
/// 승격 차례를 저장소 가지로 채울 때 같은 자리로 짝짓는다. 원본: `apBranchRoles`
pub fn branch_roles(b: &[String]) -> BranchRoles {
    BranchRoles { dev: first_of(&["dev", "develop"], b), live: first_of(&["live", "staging"], b), master: first_of(&["master", "main"], b) }
}

/// 프로젝트 설정에 승격 차례가 없을 때 저장소 가지로 채운 차례 — dev ← 자율주행 가지, live ← 자율주행 가지(dev 뒤),
/// master ← live(live 뒤). live 가 없으면 master ← 자율주행 가지(dev 뒤). 없는 자리의 단계는 없다. 원본: `apDefaultPromotion`
pub fn default_promotion(branches: &[String]) -> Vec<Step> {
    let n = branch_roles(branches);
    let mut st: Vec<Step> = Vec::new();
    if let Some(d) = &n.dev {
        st.push(Step::new("autopilot", d, None));
    }
    if let Some(l) = &n.live {
        st.push(Step::new("autopilot", l, n.dev.as_deref()));
    }
    if let Some(m) = &n.master {
        st.push(match &n.live {
            Some(l) => Step::new(l, m, Some(l)),
            None => Step::new("autopilot", m, n.dev.as_deref()),
        });
    }
    st
}

/// 바꾼 승격 차례의 고칠 것 — `branches` 는 저장소 가지 이름(묶음이면 모든 저장소에 있는 가지). 엔진 finish 의 차례 규칙과 맞춘다:
/// 자율주행 가지를 합치는 단계(from autopilot)와 기존 가지끼리 올리는 단계(from <가지>), after 는 먼저 끝나야 할 단계의 받는 가지.
/// 고칠 것이 있으면 출발을 잠근다. 원본: `apPromotionProblems`
pub fn promotion_problems(steps: &[Step], branches: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut intos: Vec<String> = Vec::new();
    let has = |v: &[String], x: &str| v.iter().any(|y| y == x);
    for (i, s) in steps.iter().enumerate() {
        let n = format!("단계 {}", i + 1);
        if !has(branches, &s.into) {
            out.push(format!("{n}: 받는 가지 {} 가 저장소에 없습니다", s.into));
        }
        if s.from != "autopilot" {
            if s.from == s.into {
                out.push(format!("{n}: 같은 가지({})끼리는 올리지 않습니다", s.into));
            } else if !has(branches, &s.from) {
                out.push(format!("{n}: 올릴 가지 {} 가 저장소에 없습니다", s.from));
            } else if !has(&intos, &s.from) {
                out.push(format!("{n}: {} 는 앞 단계가 받는 가지가 아닙니다(자율주행 가지를 먼저 그 가지에 합치는 단계를 앞에 둡니다)", s.from));
            }
        }
        if let Some(a) = &s.after {
            if *a == s.into {
                out.push(format!("{n}: 뒤 조건이 받는 가지 자신({a})입니다"));
            } else if !has(&intos, a) {
                out.push(format!("{n}: {a} 뒤라는 조건의 단계가 앞에 없습니다"));
            }
        }
        let key = format!("{}>{}", s.from, s.into);
        if has(&seen, &key) {
            let from = if s.from == "autopilot" { "자율주행 가지" } else { s.from.as_str() };
            out.push(format!("{n}: 같은 단계({from} → {})가 두 번 있습니다", s.into));
        }
        seen.push(key);
        if !has(&intos, &s.into) {
            intos.push(s.into.clone());
        }
    }
    out
}

/// 바꾸기 화면의 고를 수 있는 것 — 단계 `at` 의 받는 가지(저장소 가지에서 그 단계의 올릴 가지 말고), 올릴 것(자율주행 가지와 앞 단계가 받는 가지),
/// 뒤 조건(없음과 앞 단계가 받는 가지). `at` 이 단계 수 이상이면 새 단계(올릴 가지는 자율주행 가지)다. 원본: `apPromotionChoices`
pub fn promotion_choices(steps: &[Step], at: usize, branches: &[String]) -> PromotionChoices {
    let mut prev: Vec<String> = Vec::new();
    for s in steps.iter().take(at.min(steps.len())) {
        if !prev.contains(&s.into) {
            prev.push(s.into.clone());
        }
    }
    let from = steps.get(at).map_or("autopilot", |s| s.from.as_str());
    let mut from_list = vec!["autopilot".to_string()];
    from_list.extend(prev.iter().cloned());
    PromotionChoices { into: branches.iter().filter(|b| b.as_str() != from).cloned().collect(), from: from_list, after: prev }
}
