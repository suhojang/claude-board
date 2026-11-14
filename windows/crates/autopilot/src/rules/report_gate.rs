//! 보고서 창의 콘텐츠 규칙(바깥 요청 막기) 준비 차례. 원본: `Sources/AutopilotRules/ReportGate.swift`.
//! 보고서 창의 웹 보기에 http(s)·ws(s)·ftp(s) 요청을 막는 규칙을 붙이는데, 규칙 준비는 비동기다. 이 규칙은 창마다
//! "글을 실어도 되는가" 를 정한다:
//!  - 규칙이 준비됐으면 바로 싣는다.
//!  - 준비 중이면 기다린다(규칙 없이 싣지 않음). 끝나면 기다린 창을 모두 싣는다.
//!  - 실패하면 기다린 창에 까닭을 보이고 싣지 않는다(닫힌 쪽). 다음 창을 열거나 "다시 해 보기" 를 누르면 다시 준비한다.
//!
//! WebView2 호출·창은 앱 쪽이 쥐고, 여기는 값만 다룬다(WebKit 의 규칙 컴파일 → WebView2 요청 가로채기 등록은 앱 몫).

/// 규칙 준비 단계. 원본: `APRuleGatePhase`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGatePhase {
    /// 아직 준비하지 않음
    NotYet,
    /// 준비 중(한 번에 하나)
    Compiling,
    /// 규칙이 있음 — 창마다 붙인 뒤 싣는다
    Ready,
    /// 마지막 준비가 실패함 — 다음 요청이 다시 준비한다
    Failed,
}

/// 앱이 할 일. 원본: `APRuleGateAction`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleGateAction {
    /// 준비를 시작한다(끝나면 `compiled` 를 부른다)
    Compile,
    /// 이 창은 규칙을 기다린다(싣지 않고 기다림 글)
    Wait(String),
    /// 이 창에 규칙을 붙이고 글을 싣는다
    Load(String),
    /// 이 창은 싣지 않는다(창, 까닭) — 까닭 글과 "다시 해 보기"
    Refuse(String, String),
}

/// 원본: `APRuleGate`. Swift 의 `private(set)` 필드는 읽기 메서드로 연다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleGate {
    phase: RuleGatePhase,
    waiting: Vec<String>,
    last_error: Option<String>,
    tries: u32,
}

impl Default for RuleGate {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleGate {
    pub fn new() -> Self {
        Self { phase: RuleGatePhase::NotYet, waiting: Vec::new(), last_error: None, tries: 0 }
    }

    pub fn phase(&self) -> RuleGatePhase {
        self.phase
    }

    /// 규칙을 기다리는 창(연 차례)
    pub fn waiting(&self) -> &[String] {
        &self.waiting
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// 준비를 시작한 횟수(실패 뒤 다시 한 것 포함)
    pub fn tries(&self) -> u32 {
        self.tries
    }

    /// 창 하나가 글을 싣고 싶다(창을 열 때·같은 실행을 다시 눌러 다시 그릴 때·"다시 해 보기"). 원본: `want`
    pub fn want(&mut self, window: &str) -> Vec<RuleGateAction> {
        match self.phase {
            RuleGatePhase::Ready => vec![RuleGateAction::Load(window.to_string())],
            RuleGatePhase::Compiling => {
                self.wait_for(window);
                vec![RuleGateAction::Wait(window.to_string())]
            }
            RuleGatePhase::NotYet | RuleGatePhase::Failed => {
                self.phase = RuleGatePhase::Compiling;
                self.tries += 1;
                self.wait_for(window);
                vec![RuleGateAction::Compile, RuleGateAction::Wait(window.to_string())]
            }
        }
    }

    /// 준비가 끝났다. 되면 기다린 창을 모두 싣고, 안 되면 기다린 창에 까닭(다음 요청이 다시 준비).
    /// 준비 중이 아닐 때 온 끝(늦게 온 콜백)은 무시. 원본: `compiled(ok:error:)`
    pub fn compiled(&mut self, ok: bool, error: Option<&str>) -> Vec<RuleGateAction> {
        if self.phase != RuleGatePhase::Compiling {
            return Vec::new();
        }
        let w = std::mem::take(&mut self.waiting);
        if ok {
            self.phase = RuleGatePhase::Ready;
            self.last_error = None;
            return w.into_iter().map(RuleGateAction::Load).collect();
        }
        self.phase = RuleGatePhase::Failed;
        let why = rule_gate_why(error);
        self.last_error = Some(why.clone());
        w.into_iter().map(|window| RuleGateAction::Refuse(window, why.clone())).collect()
    }

    /// 창이 닫혔다 — 기다리던 창이면 뺀다(준비가 끝나도 싣지 않게). 원본: `closed`
    pub fn closed(&mut self, window: &str) {
        self.waiting.retain(|w| w != window);
    }

    fn wait_for(&mut self, window: &str) {
        if !self.waiting.iter().any(|w| w == window) {
            self.waiting.push(window.to_string());
        }
    }
}

/// 실패 까닭 글(창에 한 줄) — 오류 글은 160 글자까지. 원본: `apRuleGateWhy`.
/// Swift 의 `count`·`prefix` 는 확장 자소 단위, 여기는 `char`(유니코드 스칼라) 단위다. 결합 글자가 섞인 오류 글에서만 갈린다.
pub fn rule_gate_why(error: Option<&str>) -> String {
    let e = error.unwrap_or("").trim();
    let short = if e.chars().count() > 160 {
        let cut: String = e.chars().take(160).collect();
        format!("{cut}…")
    } else {
        e.to_string()
    };
    let detail = if short.is_empty() { ".".to_string() } else { format!("({short}).") };
    format!("바깥 요청 막기 규칙을 만들지 못해 보고서를 싣지 않았습니다{detail} 다시 해 보기를 누르거나 창을 다시 열면 규칙을 다시 만듭니다.")
}

/// 규칙을 기다리는 동안 창에 보이는 글. 원본: `apRuleGateWaitText`
pub const RULE_GATE_WAIT_TEXT: &str = "바깥 요청 막기 규칙을 준비하는 중입니다. 규칙이 붙은 뒤에 보고서를 싣습니다.";
