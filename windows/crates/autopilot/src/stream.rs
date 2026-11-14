//! claude `--output-format stream-json` 한 줄의 순수 해석기와 한 턴의 셈.
//! 파일·프로세스·시각을 직접 부르지 않는다(시각은 `feed` 인자). 원본은 macOS 엔진 `Sources/ClaudeAutopilot/main.swift` 의
//! `Turn`(3763-3777)·`handle`(3910-3982)·`resetsOf`(3984-3989)·`noteRate`(3991-4002)와 설계 4장 「한도 거절 가리기」·「사용률」이다.
//! 사건 하나를 읽는 일(`parse_line`)과 셈에 반영하는 일(`StreamState::feed`)을 나눠, 바깥에 할 일(진행 기록·context 기록)은 `Emit` 으로 돌려준다.
//!
//! 이식에서 원본과 다른 곳(의도): 유한하지 않은 비용 값(`"NaN"` 같은 문자열)은 무시한다. 글자 수는 Swift `Character` 가 아니라 유니코드 스칼라(`char`)로 센다.

use serde_json::{Map, Value};

/// 빈 result 를 끝으로 세지 않았다는 안내(원본 3968)
pub const EMPTY_RESULT_NOTE: &str = "빈 result(num_turns 0)를 받아 끝으로 세지 않았습니다";
/// 진행 기록에 남기는 모델 글의 길이(원본 `short` 기본값 140)
const MODEL_TEXT_CHARS: usize = 140;
/// 오류 글 길이(원본 3977)
const ERR_TEXT_CHARS: usize = 120;
/// context 기록의 도구 입력 길이(원본 3943)
const TOOL_INPUT_CHARS: usize = 600;
/// context 기록의 도구 결과 길이(원본 3955)
const TOOL_OUTPUT_CHARS: usize = 400;
/// 모델 글 꼬리로 남기는 길이
pub const TEXT_TAIL_CHARS: usize = 4000;

/// 한도 창 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    FiveHour,
    SevenDay,
}

/// `unifiedWindows` 안의 창 하나. 값이 없으면 `None`(원본은 nil 을 대입해 키를 지운다)
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub utilization: Option<f64>,
    pub resets_at: Option<f64>,
}

/// `unifiedWindows` 가 비었을 때 `rate_limit_info` 자체에 담겨 오는 사용률 한 줄(원본 3997)
#[derive(Debug, Clone, PartialEq)]
pub struct FlatUsage {
    pub kind: LimitKind,
    pub utilization: f64,
    pub resets_at: Option<f64>,
}

/// `rate_limit_event` 의 `rate_limit_info` 를 읽은 것
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RateInfo {
    /// `status == "rejected"`: 한도 거절 꼴 ①
    pub rejected: bool,
    /// 초기화 시각(초). `unifiedWindows[rateLimitType].resetsAt`, 없으면 `resetsAt`
    pub resets_at: Option<f64>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub flat: Option<FlatUsage>,
}

/// assistant 사건의 content 블록(text·tool_use 만 남긴다)
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantBlock {
    Text(String),
    ToolUse { id: String, name: String, input: Value },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    pub is_error: bool,
    /// 결과 글 전체(문자열이거나 text 조각을 줄바꿈으로 이은 것)
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Denial {
    pub tool_name: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultEvent {
    pub subtype: String,
    pub session_id: Option<String>,
    pub num_turns: i64,
    pub text: String,
    pub is_error: bool,
    pub total_cost_usd: Option<f64>,
    pub api_error_status: Option<f64>,
    pub api_error: Option<String>,
    pub denials: Vec<Denial>,
}

/// stream-json 한 줄이 뜻하는 사건. 모르는 꼴은 `Other`, 읽을 수 없는 줄은 `Garbage`
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    System { subtype: String, session_id: Option<String>, tasks: Option<usize>, attempt: Option<f64> },
    Assistant {
        /// 보조 에이전트가 아닌 본 세션의 사건(`parent_tool_use_id` 가 없거나 null)
        main: bool,
        /// `error == "rate_limit"` 또는 `api_error_status == 429`: 한도 거절 꼴 ②
        rate_limited: bool,
        /// `rate_limited` 일 때 `api_error_params.rate_limit_info` 의 초기화 시각
        limit_resets: Option<f64>,
        blocks: Vec<AssistantBlock>,
    },
    User { tool_results: Vec<ToolResultBlock> },
    RateLimit(RateInfo),
    Result(ResultEvent),
    /// JSON 객체지만 셈에 쓰는 type 이 아니다
    Other,
    /// 빈 줄·JSON 이 아닌 줄·객체가 아닌 JSON
    Garbage,
}

// ---- 읽기 ----

/// 원본 `num`: 숫자(참·거짓은 1·0)이거나 숫자로 읽히는 문자열
fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn str_of<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

/// 원본 `as? [[String: Any]]`: 원소가 모두 객체일 때만 읽고, 하나라도 아니면 통째로 없는 것으로 본다
fn objects(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    let Some(Value::Array(a)) = v else { return Vec::new() };
    let objs: Vec<_> = a.iter().filter_map(Value::as_object).collect();
    if objs.len() == a.len() { objs } else { Vec::new() }
}

/// 원본 `resetsOf`
fn resets_of(info: &Map<String, Value>) -> Option<f64> {
    if let Some(k) = str_of(info, "rateLimitType") {
        let r = info.get("unifiedWindows").and_then(Value::as_object).and_then(|w| w.get(k)).and_then(Value::as_object);
        if let Some(r) = r.and_then(|w| num(w.get("resetsAt"))) {
            return Some(r);
        }
    }
    num(info.get("resetsAt"))
}

fn window(windows: &Map<String, Value>, key: &str) -> Option<Window> {
    let f = windows.get(key)?.as_object()?;
    Some(Window { utilization: num(f.get("utilization")), resets_at: num(f.get("resetsAt")) })
}

fn rate_info(info: &Map<String, Value>) -> RateInfo {
    let empty = Map::new();
    let windows = info.get("unifiedWindows").and_then(Value::as_object).unwrap_or(&empty);
    let flat = if windows.is_empty() {
        let kind = match str_of(info, "rateLimitType") {
            Some("five_hour") => Some(LimitKind::FiveHour),
            Some("seven_day") => Some(LimitKind::SevenDay),
            _ => None,
        };
        match (num(info.get("utilization")), kind) {
            (Some(utilization), Some(kind)) => Some(FlatUsage { kind, utilization, resets_at: num(info.get("resetsAt")) }),
            _ => None,
        }
    } else {
        None
    };
    RateInfo {
        rejected: str_of(info, "status") == Some("rejected"),
        resets_at: resets_of(info),
        five_hour: window(windows, "five_hour"),
        seven_day: window(windows, "seven_day"),
        flat,
    }
}

fn parse_assistant(o: &Map<String, Value>) -> StreamEvent {
    let empty = Map::new();
    let msg = o.get("message").and_then(Value::as_object).unwrap_or(&empty);
    let main = matches!(o.get("parent_tool_use_id"), None | Some(Value::Null));
    let rate_limited = str_of(o, "error") == Some("rate_limit") || num(o.get("api_error_status")) == Some(429.0);
    let limit_resets = if rate_limited {
        o.get("api_error_params")
            .and_then(Value::as_object)
            .and_then(|p| p.get("rate_limit_info"))
            .and_then(Value::as_object)
            .and_then(resets_of)
    } else {
        None
    };
    let blocks = objects(msg.get("content"))
        .into_iter()
        .filter_map(|b| match str_of(b, "type") {
            Some("text") => Some(AssistantBlock::Text(str_of(b, "text").unwrap_or("").to_string())),
            Some("tool_use") => Some(AssistantBlock::ToolUse {
                id: str_of(b, "id").unwrap_or("").to_string(),
                name: str_of(b, "name").unwrap_or("?").to_string(),
                input: match b.get("input") {
                    Some(v @ Value::Object(_)) => v.clone(),
                    _ => Value::Object(Map::new()),
                },
            }),
            _ => None,
        })
        .collect();
    StreamEvent::Assistant { main, rate_limited, limit_resets, blocks }
}

fn parse_user(o: &Map<String, Value>) -> StreamEvent {
    let empty = Map::new();
    let msg = o.get("message").and_then(Value::as_object).unwrap_or(&empty);
    let tool_results = objects(msg.get("content"))
        .into_iter()
        .filter(|b| str_of(b, "type") == Some("tool_result"))
        .map(|b| {
            let text = match b.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(a @ Value::Array(_)) => {
                    // 원본: 원소가 모두 객체일 때만 읽고, text 가 있는 것만 줄바꿈으로 잇는다
                    objects(Some(a)).into_iter().filter_map(|c| str_of(c, "text")).collect::<Vec<_>>().join("\n")
                }
                _ => String::new(),
            };
            ToolResultBlock {
                tool_use_id: str_of(b, "tool_use_id").unwrap_or("").to_string(),
                is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                text,
            }
        })
        .collect();
    StreamEvent::User { tool_results }
}

fn parse_result(o: &Map<String, Value>) -> StreamEvent {
    let denials = objects(o.get("permission_denials"))
        .into_iter()
        .map(|d| Denial {
            tool_name: str_of(d, "tool_name").unwrap_or("?").to_string(),
            tool_input: match d.get("tool_input") {
                Some(v @ Value::Object(_)) => v.clone(),
                _ => Value::Object(Map::new()),
            },
        })
        .collect();
    StreamEvent::Result(ResultEvent {
        subtype: str_of(o, "subtype").unwrap_or("").to_string(),
        session_id: str_of(o, "session_id").map(str::to_string),
        // f64 → i64 는 포화 변환이라 NaN·무한대·아주 큰 값에도 멈추지 않는다
        num_turns: num(o.get("num_turns")).unwrap_or(0.0) as i64,
        text: str_of(o, "result").unwrap_or("").to_string(),
        is_error: o.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        total_cost_usd: num(o.get("total_cost_usd")),
        api_error_status: num(o.get("api_error_status")),
        api_error: str_of(o, "api_error").map(str::to_string),
        denials,
    })
}

/// stream-json 한 줄을 읽는다. 어떤 줄이 와도 멈추지 않는다
/// 원본: `handle` 의 첫머리(3913-3915)
pub fn parse_line(line: &str) -> StreamEvent {
    let line = line.trim_end_matches(['\r', '\n']);
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(line) else { return StreamEvent::Garbage };
    match str_of(&o, "type").unwrap_or("") {
        "system" => StreamEvent::System {
            subtype: str_of(&o, "subtype").unwrap_or("").to_string(),
            session_id: str_of(&o, "session_id").map(str::to_string),
            tasks: o.get("tasks").and_then(Value::as_array).map(Vec::len),
            attempt: num(o.get("attempt")),
        },
        "assistant" => parse_assistant(&o),
        "user" => parse_user(&o),
        "rate_limit_event" => {
            let empty = Map::new();
            StreamEvent::RateLimit(rate_info(o.get("rate_limit_info").and_then(Value::as_object).unwrap_or(&empty)))
        }
        "result" => parse_result(&o),
        _ => StreamEvent::Other,
    }
}

// ---- 글 다루기 ----

/// 앞 `n` 글자
fn prefix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// 원본 `short`: 처음으로 빈 줄이 아닌 줄을 `n` 글자까지, 넘으면 `…` 를 붙인다
fn short(s: &str, n: usize) -> String {
    let line = s
        .split(['\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}'])
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    match line.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

fn keep_tail(s: &mut String, n: usize) {
    let count = s.chars().count();
    if count > n {
        if let Some((cut, _)) = s.char_indices().nth(count - n) {
            s.drain(..cut);
        }
    }
}

/// 원본 `r3`: 소수 셋째 자리까지
fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// context 기록에 남기는 도구 입력(JSON 글의 앞 600자, 원본 3943)
pub fn tool_input_summary(input: &Value) -> String {
    prefix_chars(&input.to_string(), TOOL_INPUT_CHARS).to_string()
}

// ---- 셈 ----

/// 계정 사용률(0~1, 0.01 단위)과 초기화 시각(유닉스 초). 원본 `noteRate` 가 run.json `usage` 에 남기는 꼴(`from` 은 늘 stream)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamUsage {
    pub five_hour: Option<f64>,
    pub five_hour_resets: Option<f64>,
    pub seven_day: Option<f64>,
    pub seven_day_resets: Option<f64>,
    pub at: f64,
}

/// 도구 호출 하나(증거·diff 단계가 어떤 파일을 건드렸는지 보는 재료)
#[derive(Debug, Clone, PartialEq)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub main: bool,
}

/// `feed` 가 바깥에 할 일로 돌려주는 것. 글을 만드는 일(쉬운 말 진행 기록)은 받는 쪽이 한다
#[derive(Debug, Clone, PartialEq)]
pub enum Emit {
    /// context 기록의 note (빈 result, API 다시 시도)
    Note(String),
    /// 본 세션 모델 글 한 줄(`short`, 바로 앞과 같으면 내지 않음)
    ModelText(String),
    /// 모아 둔 "살펴봄"(읽기·찾기) 수를 한 줄로 낼 때
    Looked(u32),
    /// 도구 호출(context 기록용). 읽기·찾기(`Read`·`Glob`·`Grep`·`LS`)가 아니면 본 세션 것은 앞에 `Looked` 가 먼저 온다
    ToolUse(ToolUse),
    /// 도구 결과(context 기록용, `output` 은 앞 400자)
    ToolResult { tool_use_id: String, is_error: bool, output: String },
    /// 허용 범위 밖이라 거부된 도구
    Denial(Denial),
    /// 엔진이 정한 session id 로 열린 세션임을 처음 확인했다
    SessionStarted,
}

/// 한 claude 프로세스의 턴 동안의 셈. 필드 이름은 원본 `Turn`(3764-3777) 그대로다
#[derive(Debug, Clone, PartialEq)]
pub struct StreamState {
    expected_session_id: String,
    /// 마지막으로 본 session id(init 과 result)
    pub session_id: Option<String>,
    /// init 의 session id 가 기대한 것과 같았다(원본 `u["session_started"]`)
    pub session_started: bool,
    /// 진짜 result 를 받았다. num_turns 0 인 빈 result 는 세지 않는다
    pub result_seen: bool,
    /// result 뒤 새 턴이 시작됐다(백그라운드 작업 알림 등)
    pub open: bool,
    pub last_error: bool,
    /// 돌고 있는 백그라운드 작업 수
    pub bg: usize,
    /// 한도 거절을 받았다
    pub limit: bool,
    pub limit_resets: Option<f64>,
    /// 비용 상한(`error_max_budget_usd`)에 닿았다
    pub budget: bool,
    /// 이 프로세스의 누적 비용(result 의 `total_cost_usd`, 프로세스마다 0 부터)
    pub cost: f64,
    /// 마지막으로 읽을 수 있는 줄이 온 시각
    pub last_line: f64,
    /// 바로 앞에 낸 모델 글 한 줄(되풀이 방지)
    pub last_text: String,
    /// 모아 쓰는 "살펴봄" 수
    pub looked: u32,
    pub err_text: String,
    /// 단위 전체의 턴 수(result 의 `num_turns` 합, 원본 `u["turns"]`). 프로세스를 다시 띄워도 이어진다
    pub turns_total: i64,
    /// 본 세션 모델 글의 꼬리(마지막 `TEXT_TAIL_CHARS` 글자)
    pub text_tail: String,
    /// 읽은 도구 호출 전부. 증거 단계가 `take_tool_uses` 로 가져간다
    pub tool_uses: Vec<ToolUse>,
    /// 가장 최근 `rate_limit_event` 에서 읽은 계정 사용률. 프로세스를 다시 띄워도 이어진다
    pub usage: Option<StreamUsage>,
}

impl StreamState {
    /// `expected_session_id`: 엔진이 이 프로세스에 정해 준 session id. `now`: 지금 시각(초)
    pub fn new(expected_session_id: &str, now: f64) -> Self {
        StreamState {
            expected_session_id: expected_session_id.to_string(),
            session_id: None,
            session_started: false,
            result_seen: false,
            open: false,
            last_error: false,
            bg: 0,
            limit: false,
            limit_resets: None,
            budget: false,
            cost: 0.0,
            last_line: now,
            last_text: String::new(),
            looked: 0,
            err_text: String::new(),
            turns_total: 0,
            text_tail: String::new(),
            tool_uses: Vec::new(),
            usage: None,
        }
    }

    /// 새 프로세스를 띄울 때: 원본의 `Turn()` 새로 만들기. 단위에 걸친 값(턴 수·사용률·도구 호출·글 꼬리·세션 확인)은 둔다
    pub fn restart_turn(&mut self, now: f64) {
        self.result_seen = false;
        self.open = false;
        self.last_error = false;
        self.bg = 0;
        self.limit = false;
        self.limit_resets = None;
        self.budget = false;
        self.cost = 0.0;
        self.last_line = now;
        self.last_text.clear();
        self.looked = 0;
        self.err_text.clear();
    }

    pub fn take_tool_uses(&mut self) -> Vec<ToolUse> {
        std::mem::take(&mut self.tool_uses)
    }

    /// 모아 둔 "살펴봄" 수를 꺼내며 0 으로 돌린다(원본 `flushLooked`)
    pub fn take_looked(&mut self) -> u32 {
        std::mem::take(&mut self.looked)
    }

    fn flush_looked(&mut self, out: &mut Vec<Emit>) {
        let n = self.take_looked();
        if n > 0 {
            out.push(Emit::Looked(n));
        }
    }

    /// 원본 `noteRate`(3992-4002)
    fn note_rate(&mut self, info: &RateInfo, now: f64) {
        let mut u = self.usage.take().unwrap_or_default();
        if let Some(f) = &info.five_hour {
            u.five_hour = f.utilization;
            u.five_hour_resets = f.resets_at;
        }
        if let Some(s) = &info.seven_day {
            u.seven_day = s.utilization;
            u.seven_day_resets = s.resets_at;
        }
        if let Some(f) = &info.flat {
            match f.kind {
                LimitKind::FiveHour => {
                    u.five_hour = Some(f.utilization);
                    u.five_hour_resets = f.resets_at;
                }
                LimitKind::SevenDay => {
                    u.seven_day = Some(f.utilization);
                    u.seven_day_resets = f.resets_at;
                }
            }
        }
        u.at = r3(now);
        self.usage = Some(u);
    }

    /// 사건 하나를 셈에 넣는다. 원본 `handle` 의 분기(3916-3981)를 그대로 따른다
    pub fn feed(&mut self, ev: &StreamEvent, now: f64) -> Vec<Emit> {
        let mut out = Vec::new();
        if matches!(ev, StreamEvent::Garbage) {
            return out;
        }
        self.last_line = now;
        // result 뒤에 모델이 다시 움직이면 그 턴이 끝날 때까지 기다린다(원본 3918)
        let opens = match ev {
            StreamEvent::Assistant { .. } | StreamEvent::User { .. } => true,
            StreamEvent::System { subtype, .. } => subtype == "init",
            _ => false,
        };
        if opens {
            self.open = true;
        }
        match ev {
            StreamEvent::System { subtype, session_id, tasks, attempt } => {
                if subtype == "init" {
                    if let Some(sid) = session_id.as_deref().filter(|s| !s.is_empty()) {
                        self.session_id = Some(sid.to_string());
                    }
                    if session_id.as_deref() == Some(self.expected_session_id.as_str()) && !self.session_started {
                        self.session_started = true;
                        out.push(Emit::SessionStarted);
                    }
                }
                if subtype == "background_tasks_changed" {
                    self.bg = tasks.unwrap_or(0);
                }
                if subtype == "api_retry" {
                    let n = attempt.map_or_else(|| "?".to_string(), |a| (a as i64).to_string());
                    out.push(Emit::Note(format!("API 다시 시도 {n}번째")));
                }
            }
            StreamEvent::Assistant { main, rate_limited, limit_resets, blocks } => {
                if *rate_limited {
                    self.limit = true;
                    if let Some(r) = limit_resets {
                        self.limit_resets = Some(*r);
                    }
                }
                for b in blocks {
                    match b {
                        AssistantBlock::Text(text) => {
                            if !*main {
                                continue;
                            }
                            if !text.is_empty() {
                                if !self.text_tail.is_empty() {
                                    self.text_tail.push('\n');
                                }
                                self.text_tail.push_str(text);
                                keep_tail(&mut self.text_tail, TEXT_TAIL_CHARS);
                            }
                            let s = short(text, MODEL_TEXT_CHARS);
                            if !s.is_empty() && s != self.last_text {
                                self.flush_looked(&mut out);
                                self.last_text = s.clone();
                                out.push(Emit::ModelText(s));
                            }
                        }
                        AssistantBlock::ToolUse { id, name, input } => {
                            let tu = ToolUse { id: id.clone(), name: name.clone(), input: input.clone(), main: *main };
                            if *main {
                                if matches!(name.as_str(), "Read" | "Glob" | "Grep" | "LS") {
                                    self.looked += 1;
                                } else {
                                    self.flush_looked(&mut out);
                                }
                            }
                            self.tool_uses.push(tu.clone());
                            out.push(Emit::ToolUse(tu));
                        }
                    }
                }
            }
            StreamEvent::User { tool_results } => {
                for r in tool_results {
                    out.push(Emit::ToolResult {
                        tool_use_id: r.tool_use_id.clone(),
                        is_error: r.is_error,
                        output: prefix_chars(&r.text, TOOL_OUTPUT_CHARS).to_string(),
                    });
                }
            }
            StreamEvent::RateLimit(info) => {
                self.note_rate(info, now);
                if info.rejected {
                    self.limit = true;
                    self.limit_resets = info.resets_at.or(self.limit_resets);
                }
            }
            StreamEvent::Result(r) => {
                if let Some(sid) = r.session_id.as_deref().filter(|s| !s.is_empty()) {
                    self.session_id = Some(sid.to_string());
                }
                if r.num_turns == 0 && r.text.is_empty() && !r.is_error {
                    // 이어 열기 직후 등에 오는 빈 result — 끝으로 세지 않는다(원본 3965-3970)
                    self.open = false;
                    out.push(Emit::Note(EMPTY_RESULT_NOTE.to_string()));
                    return out;
                }
                self.result_seen = true;
                self.open = false;
                self.last_error = r.is_error;
                if let Some(c) = r.total_cost_usd.filter(|c| c.is_finite()) {
                    self.cost = self.cost.max(c);
                }
                self.turns_total = self.turns_total.saturating_add(r.num_turns);
                if r.subtype == "error_max_budget_usd" {
                    self.budget = true;
                }
                // 한도 거절 꼴 ③: subtype 은 success 인데 is_error true 로 온다(실측, 원본 3975-3976)
                if r.is_error && (r.api_error_status == Some(429.0) || r.api_error.as_deref() == Some("usage_limit_reached")) {
                    self.limit = true;
                }
                if r.is_error {
                    self.err_text = short(if r.text.is_empty() { &r.subtype } else { &r.text }, ERR_TEXT_CHARS);
                }
                self.flush_looked(&mut out);
                out.extend(r.denials.iter().cloned().map(Emit::Denial));
            }
            StreamEvent::Other | StreamEvent::Garbage => {}
        }
        out
    }
}
