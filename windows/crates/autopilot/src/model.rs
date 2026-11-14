//! 실행 폴더 파일의 serde 꼴 — macOS 엔진이 쓴 실행 폴더를 읽고 다시 써도 값과 키 순서가 그대로다.
//!
//! 정본은 `docs/자율주행/설계.md` 5.2. 읽고 쓰는 칸만 이름 붙여 두고 나머지(모르는 칸·macOS 만 쓰는 칸)는
//! 각 구조체의 `extra` 에 그대로 담아 돌려 쓴다. 규칙 셋:
//!
//! - **칸의 세 상태** — 없음·`null`·값을 `Slot` 이 가른다. macOS 는 `wait_until`·`unit` 등을 `NSNull` 로 쓰므로
//!   `Option` 으로 읽으면 `null` 칸이 다시 쓸 때 사라진다.
//! - **키 순서** — `Ordered<P>` 가 읽은 순서를 기억해 다시 쓸 때 그 순서로 낸다(macOS 는 키를 정렬해 쓴다).
//!   읽은 적 없는 새 칸은 끝에 붙는다. 새로 만드는 값은 선언 순서(이름 순)로 나가 macOS 가 쓴 것과 같은 순서가 된다.
//! - **숫자** — 시각은 유닉스 초(f64)지만 `1761000000` 과 `1761000000.0` 을 가르려고 `Num` 이 읽은 글자 그대로 쥔다.
//!
//! 파일 쓰기(임시 파일 + 바꿔 넣기)는 `RunFiles` 포트의 몫이고, 여기는 글 ↔ 값 변환만 한다.
//! 줄 띄움은 serde_json 꼴(`"k": v`)이라 macOS Foundation 의 `"k" : v` 와 공백이 다를 수 있다. JSON 값과 키 순서는 같다.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Number, Value};
use std::ops::{Deref, DerefMut};

// ── 칸 하나의 세 상태 ─────────────────────────────

/// 칸이 없음(`Absent`)·`null`·값(`Val`) 중 어느 것인가. 없는 칸은 쓸 때 건너뛰고 `null` 은 `null` 로 쓴다.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Slot<T> {
    #[default]
    Absent,
    Null,
    Val(T),
}

impl<T> From<T> for Slot<T> {
    fn from(v: T) -> Self {
        Slot::Val(v)
    }
}

impl<T> Slot<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Slot::Absent)
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Slot::Null)
    }
    /// 값이 있을 때만 `Some`. 없음과 `null` 은 둘 다 `None`
    pub fn get(&self) -> Option<&T> {
        match self {
            Slot::Val(v) => Some(v),
            _ => None,
        }
    }
    pub fn set(&mut self, v: T) {
        *self = Slot::Val(v);
    }
    /// macOS 의 `NSNull` 에 해당하는 쓰기
    pub fn set_null(&mut self) {
        *self = Slot::Null;
    }
    /// 칸을 지운다(키가 사라진다)
    pub fn clear(&mut self) {
        *self = Slot::Absent;
    }
}

impl<T: Serialize> Serialize for Slot<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Val(v) => v.serialize(s),
            // 없음은 `skip_serializing_if` 가 거르므로 여기 오지 않는다
            Slot::Null | Slot::Absent => s.serialize_none(),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Slot<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            Some(v) => Slot::Val(v),
            None => Slot::Null,
        })
    }
}

// ── 숫자 ───────────────────────────────────────

/// 숫자 칸. 읽은 JSON 값을 그대로 쥐어 정수·실수 표기가 다시 쓸 때 바뀌지 않는다.
/// `Value` 라서 숫자가 아닌 것이 와도 읽기는 실패하지 않는다(`as_f64` 가 None).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Num(Value);

impl Num {
    /// 정수로 떨어지는 값은 정수로 쓴다(macOS 가 `0`·`1761000000` 으로 쓰는 것과 맞춘다). 유한하지 않으면 `null`
    pub fn from_f64(x: f64) -> Self {
        if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 {
            return Num(Value::from(x as i64));
        }
        Num(Number::from_f64(x).map_or(Value::Null, Value::Number))
    }
    /// 원본: `r3` — 시각·초를 1/1000 단위로 둥글린다
    pub fn r3(x: f64) -> Self {
        Self::from_f64((x * 1000.0).round() / 1000.0)
    }
    /// 숫자, 또는 숫자로 읽히는 글자(원본 `num`)
    pub fn as_f64(&self) -> Option<f64> {
        match &self.0 {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn raw(&self) -> &Value {
        &self.0
    }
}

impl From<f64> for Num {
    fn from(x: f64) -> Self {
        Num::from_f64(x)
    }
}

// ── 읽은 키 순서를 기억하는 싸개 ────────────────────

/// 본체 `P` 와 읽은 키 순서. 다시 쓸 때 그 순서로 내고, 읽은 적 없는 키는 끝에 붙인다.
/// 본체에 `Deref` 되므로 칸은 `file.state` 로 바로 쓴다.
#[derive(Debug, Clone, Default)]
pub struct Ordered<P> {
    body: P,
    order: Vec<String>,
}

impl<P> Ordered<P> {
    /// 새로 만드는 값 — 순서 기억이 없어 본체의 선언 순서로 나간다
    pub fn new(body: P) -> Self {
        Ordered { body, order: Vec::new() }
    }
    pub fn into_inner(self) -> P {
        self.body
    }
}

impl<P> From<P> for Ordered<P> {
    fn from(body: P) -> Self {
        Ordered::new(body)
    }
}

impl<P> Deref for Ordered<P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.body
    }
}

impl<P> DerefMut for Ordered<P> {
    fn deref_mut(&mut self) -> &mut P {
        &mut self.body
    }
}

/// 순서 기억은 값이 아니므로 견줄 때 보지 않는다
impl<P: PartialEq> PartialEq for Ordered<P> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body
    }
}

impl<P: Serialize> Serialize for Ordered<P> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let value = serde_json::to_value(&self.body).map_err(serde::ser::Error::custom)?;
        match value {
            Value::Object(m) => reorder(m, &self.order).serialize(s),
            other => other.serialize(s),
        }
    }
}

impl<'de, P: DeserializeOwned> Deserialize<'de> for Ordered<P> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let map = Map::<String, Value>::deserialize(d)?;
        let order = map.keys().cloned().collect();
        let body = serde_json::from_value(Value::Object(map)).map_err(serde::de::Error::custom)?;
        Ok(Ordered { body, order })
    }
}

/// `order` 에 있는 키를 그 차례로, 나머지는 원래 차례대로 뒤에. (`Map::remove` 는 순서를 흩으므로 쓰지 않는다)
fn reorder(map: Map<String, Value>, order: &[String]) -> Map<String, Value> {
    let mut out = Map::with_capacity(map.len());
    for k in order {
        if let Some(v) = map.get(k) {
            out.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in map {
        if !out.contains_key(&k) {
            out.insert(k, v);
        }
    }
    out
}

// ── 구조체 정의 도우미 ──────────────────────────────

/// 칸마다 `Slot<T>` 로 만들고 모르는 칸을 `extra` 에 담는 구조체를 정의한다.
/// 칸 선언 순서가 새로 만든 값의 키 순서다 — macOS 가 키를 정렬해 쓰므로 이름 순으로 적는다.
macro_rules! slots {
    ($(#[$m:meta])* pub struct $name:ident { $( $(#[$fm:meta])* $f:ident : $t:ty ),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            $(
                $(#[$fm])*
                #[serde(default, skip_serializing_if = "Slot::is_absent")]
                pub $f: Slot<$t>,
            )*
            /// Windows 엔진이 이름 붙이지 않은 칸 — 읽은 그대로 다시 쓴다
            #[serde(flatten)]
            pub extra: Map<String, Value>,
        }
    };
}

// ── run.json ──────────────────────────────────

slots! {
    /// `run.json` — 출발 화면이 처음 설정을 쓰고 엔진이 상태를 이어 쓴다(설계 5.2 표).
    /// 설정 칸(`allowed_tools`·`write_roots` …)은 이름 붙이지 않고 `extra` 로 둔다. 엔진은 고정 설정에서 읽는다.
    pub struct Run {
        caps: Value,
        control_last: String,
        /// 마지막으로 받은 control 요청의 ts
        control_seen: Num,
        created: Num,
        dev: bool,
        ended: Num,
        engine_pid: i64,
        /// **Windows 전용.** 고른 실행 환경. macOS 는 모르는 칸이라 읽을 때 지나친다
        exec_env: Ordered<ExecEnv>,
        last_activity: Num,
        launchd_label: String,
        model: String,
        project: String,
        project_root: String,
        reason: String,
        sessions: Vec<String>,
        started: Num,
        /// `starting`·`running`·`waiting_limit`·`waiting_stopline`·`waiting_precheck`·`paused`·`taken_over`·`stopped`·`done`·`error`
        state: String,
        stopline: Value,
        title: String,
        /// 지금 단위 번호(1부터). 출발 직후는 `null`
        unit: u32,
        unit_caps: Value,
        updated: Num,
        usage: Value,
        /// 대기 끝 시각. 대기가 아니면 `null`
        wait_until: Num,
        worktree: Value,
        /// 끝 정리를 마친 시각
        wrapped: Num,
    }
}

/// `run.json` 의 `exec_env` — **Windows 전용**. `key` 는 `EnvChoice::key()`(`wsl:Ubuntu`·`pwsh` …), `kind` 는 `EnvKind::id()`.
/// 샌드박스를 못 거는 환경에서 사람이 샌드박스 없이 돌리기로 동의한 기록이 `unsandboxed_ok` 다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecEnv {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
    pub key: String,
    pub kind: String,
    /// 출발 확인에서 잰 값 — 이 환경에서 Claude Code 샌드박스가 걸리는가
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsandboxed_ok: Option<UnsandboxedOk>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 샌드박스 없이 돌리기로 동의한 때(유닉스 초)와 동의한 쪽(`board`·`cli` 등 자유 글)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsandboxedOk {
    pub at: Num,
    pub by: String,
}

// ── queue.json · pinned/frozen.json 의 단위 정의 ──────

slots! {
    /// `queue.json` 의 단위 하나(엔진이 출발 때 동결, `status` 는 엔진이 이어 씀). `pinned/frozen.json` 의 `units[]` 도 같은 꼴이다.
    pub struct QueueUnit {
        branch: String,
        caps: Value,
        check: String,
        context: String,
        cwd: String,
        /// 기대는 앞선 단위 `id` 배열
        depends: Vec<String>,
        est_min: Num,
        id: String,
        kind: String,
        prompt: String,
        review: bool,
        scenarios: Value,
        source_key: String,
        /// `pending`·`running`·`done`·`capped`·`failed`·`skipped`·`stopped` …(`engine.json` 과 같은 값)
        status: String,
        story: String,
        tickets: Vec<String>,
        title: String,
    }
}

// ── units/<n>/engine.json ─────────────────────────

slots! {
    /// `units/<n>/engine.json` — 엔진 비공개 `engine/unit-<n>.json` 의 사본(보드가 그린다).
    pub struct UnitProgress {
        active_sec: Num,
        branch: String,
        caps: Value,
        checks: Value,
        claude_runs: Num,
        cost_done: Num,
        cost_usd: Num,
        denials: Value,
        directions: Value,
        ended: Num,
        errors: Num,
        first_message: String,
        git: Value,
        id: String,
        limits: Value,
        merge: Value,
        next_message: String,
        /// `model`·`action`·`check`·`limit_wait`·`end`
        phase: String,
        procs: Value,
        reason: String,
        restart_cleanup: Value,
        result_ok: bool,
        session_id: String,
        session_started: bool,
        sessions: Value,
        started: Num,
        status: String,
        takeover: Value,
        takeovers: Value,
        title: String,
        /// 판정을 다시 한 횟수(원본 키 `try`)
        r#try: Num,
        turns: Num,
        unit: u32,
        updated: Num,
        wait_until: Num,
        /// 단위를 시작할 때의 주간 사용률. 모르면 `null`
        weekly_base: Num,
        weekly_last: Num,
        weekly_pp: Num,
        weekly_pp_done: Num,
    }
}

// ── control.json ─────────────────────────────────

slots! {
    /// `control.json` — 보드가 쓰고 엔진이 1초마다 보다가 받으면 지운다.
    /// `action` 은 `pause`·`resume`·`stop`·`takeover`·`giveback`·`restart_from_intake`. `ts` 가 없으면 파일 mtime 으로 갈음한다(원본 `apControlNew`).
    pub struct ControlRequest {
        /// 돌려주기 카드 답 `takeover-harness-<지문>`·`takeover-scenarios-<지문>` 목록
        accept: Vec<String>,
        action: String,
        ts: Num,
    }
}

// ── events.jsonl 한 줄 ────────────────────────────

slots! {
    /// `events.jsonl` 의 한 줄. `who` 는 `board`·`model`·`engine`, `ts` 는 유닉스 초.
    pub struct EventLine {
        text: String,
        ts: Num,
        who: String,
    }
}

// ── pinned/frozen.json 껍데기 ──────────────────────

slots! {
    /// `pinned/frozen.json` — 출발 때 고정한 실행 설정. 엔진이 다시 뜰 때 해시로 확인한다.
    /// 여기는 껍데기뿐이다: 단위 정의와 자주 읽는 몇 칸만 이름 붙이고 나머지는 `extra` 로 돌려 쓴다.
    /// 해시는 파일 바이트로 재므로 이 파일은 고쳐 쓰지 않는다(읽기만 하고, 다시 쓸 일이 있어도 값은 보존된다).
    pub struct Frozen {
        caps: Value,
        dev: bool,
        effort: String,
        frozen_at: Num,
        id: String,
        model: String,
        project: String,
        read_deny: Vec<String>,
        stopline: Value,
        title: String,
        units: Vec<Ordered<QueueUnit>>,
        version: Num,
        write_roots: Vec<String>,
    }
}

// ── 파일 꼴 이름 ─────────────────────────────────

pub type RunFile = Ordered<Run>;
pub type QueueFile = Vec<Ordered<QueueUnit>>;
pub type UnitProgressFile = Ordered<UnitProgress>;
pub type ControlFile = Ordered<ControlRequest>;
pub type EventFile = Ordered<EventLine>;
pub type FrozenFile = Ordered<Frozen>;

// ── 글 ↔ 값 ──────────────────────────────────────

/// 파일 바이트 → 값. 모르는 칸은 `extra` 에 남는다. 꼴이 틀리면(최상위가 객체가 아님, 칸 종류가 다름) 오류
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// 값 → 들여쓰기 2칸 JSON + 줄바꿈 하나(macOS `writeJSON` 과 같이 끝에 `\n`)
pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

/// 값 → 한 줄 JSON(줄바꿈 없음). `events.jsonl`·`context.jsonl` 에 덧붙일 때 쓴다
pub fn to_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}
