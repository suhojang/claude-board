//! 계정 전환 — `AccountStore` (`ClaudeBoard.swift AccountStore`, SPEC §5.5).
//!
//! macOS 키체인 대신 지금 로그인은 `~/.claude/.credentials.json`, 저장한 계정 토큰은
//! `notifier/accounts/vault/<계정 id 의 sha256 앞 16자>.bin`(Windows 는 DPAPI 로 암호화)에 둔다.
//! 화면에 보일 정보는 macOS 와 같은 형식의 `notifier/accounts.json`.
//! 실행 중인 세션은 요청을 보낼 때마다 로그인 정보를 다시 읽으므로 모든 세션이 다음 요청부터 새 계정으로 이어 간다.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use claude_board_core::{clock, fileio, paths, Record};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::board::Board;
use crate::model::{AccountView, AccountsView, Notice};
use crate::util::{board_log, mtime, num, obj, short_id};

/// Claude Code 가 다른 계정으로 로그인할 때 `~/.claude.json` 에서 비우는 계정별 캐시(2.1.300 기준)
pub const ACCOUNT_CACHES: [&str; 16] = [
    "additionalModelOptionsCache",
    "additionalModelOptionsAnsweredAt",
    "additionalModelCostsCache",
    "modelAccessCache",
    "orgModelDefaultCache",
    "cachedArtifactRoster",
    "artifactRosterDenied",
    "lastSeenOrgDefaultUpdatedAt",
    "clientDataCache",
    "clientDataCacheSlots",
    "autoCompactWindowsCache",
    "cachedUsageUtilization",
    "metricsStatusCache",
    "metricsStatusCacheByPrincipal",
    "githubWebConnectionStatusCache",
    "startupPrefetchedAt",
];

/// 계정 파일 위치 — 시험에서는 가짜 폴더를 넣는다.
#[derive(Debug, Clone)]
pub struct AccountPaths {
    pub claude_dir: PathBuf,
    pub claude_json: PathBuf,
    pub credentials: PathBuf,
    pub meta: PathBuf,
    /// `notifier/accounts` — `login-*` 자리와 `vault/`
    pub root: PathBuf,
}

impl AccountPaths {
    pub fn system() -> Self {
        AccountPaths {
            claude_dir: paths::claude_dir(),
            claude_json: paths::claude_json(),
            credentials: paths::credentials_file(),
            meta: paths::notifier_dir().join("accounts.json"),
            root: paths::notifier_dir().join("accounts"),
        }
    }

    fn vault(&self, id: &str) -> PathBuf {
        let hex: String = Sha256::digest(id.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        self.root.join("vault").join(format!("{}.bin", &hex[..16]))
    }
}

/// 저장해 둔 Claude 계정 — 토큰은 금고 파일에만 두고 여기에는 화면에 보일 정보만 담는다.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// accountUuid|organizationUuid — 같은 사람도 조직이 다르면 토큰이 따로다
    pub id: String,
    /// `~/.claude.json` 의 oauthAccount 그대로 — 전환할 때 이 값으로 바꿔 넣는다
    pub oauth: Record,
    /// 이 시각(유닉스 초)이 지나면 다시 로그인해야 한다
    pub refresh_expires: Option<f64>,
}

impl Account {
    pub fn new(oauth: &Record, refresh_expires: Option<f64>) -> Option<Self> {
        let uuid = oauth.get("accountUuid").and_then(Value::as_str).filter(|u| !u.is_empty())?;
        let org = oauth.get("organizationUuid").and_then(Value::as_str).unwrap_or("");
        Some(Account { id: format!("{uuid}|{org}"), oauth: oauth.clone(), refresh_expires })
    }

    fn str(&self, key: &str) -> &str {
        self.oauth.get(key).and_then(Value::as_str).unwrap_or("")
    }

    pub fn email(&self) -> String {
        self.oauth.get("emailAddress").and_then(Value::as_str).unwrap_or("?").to_string()
    }

    /// 요금제 — organizationType(claude_ 접두를 벗겨 첫 글자를 크게)과 한도 등급(…_<n>x)을 이어 붙인다.
    /// 빈 조각은 건너뛴다 — 등급만 있으면 "5x", 둘 다 있으면 "Max 5x", 종류만 있으면 "Pro"(§plan 행 #9a).
    pub fn plan(&self) -> String {
        let t = self.str("organizationType").replace("claude_", "");
        let mut chars = t.chars();
        let mut parts: Vec<String> = vec![chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()];
        let tier = self.str("organizationRateLimitTier");
        if let Some(i) = tier.rfind('_') {
            let n = &tier[i + 1..];
            if n.len() > 1 && n.ends_with('x') && n[..n.len() - 1].bytes().all(|c| c.is_ascii_digit()) {
                parts.push(n.to_string());
            }
        }
        parts.retain(|p| !p.is_empty());
        parts.join(" ")
    }

    pub fn expired(&self, now: f64) -> bool {
        self.refresh_expires.is_some_and(|e| e < now)
    }

    pub fn view(&self, now: f64) -> AccountView {
        AccountView { id: self.id.clone(), email: self.email(), org: self.str("organizationName").to_string(), plan: self.plan(), expired: self.expired(now) }
    }
}

// ── 파일 조작 ────────────────────────────────────────────────

/// `~/.claude.json` 같은 파일의 oauthAccount.
pub fn oauth_account(path: &Path) -> Option<Record> {
    obj(fileio::read_record(path)?.get("oauthAccount")).cloned()
}

/// `.credentials.json` 의 claudeAiOauth.
fn claude_ai_oauth(path: &Path) -> Option<Record> {
    obj(fileio::read_record(path)?.get("claudeAiOauth")).cloned()
}

/// 이메일·토큰이 든 파일 — 쓴 뒤 나만 읽게(유닉스 0600; Windows 는 사용자 프로필 폴더 ACL 을 따른다).
fn write_private(path: &Path, prefix: &str, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fileio::write_atomic(path, prefix, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn vault_write(p: &AccountPaths, id: &str, plain: &[u8]) -> Result<(), String> {
    let sealed = crate::win::protect(plain).ok_or("로그인 정보를 암호화하지 못했습니다")?;
    write_private(&p.vault(id), "vault", &sealed).map_err(|e| format!("로그인 정보를 저장하지 못했습니다: {e}"))
}

fn vault_read(p: &AccountPaths, id: &str) -> Option<Record> {
    let plain = crate::win::unprotect(&fs::read(p.vault(id)).ok()?)?;
    match serde_json::from_slice(&plain).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn read_meta(path: &Path) -> Vec<Record> {
    fileio::read_record(path)
        .and_then(|j| j.get("accounts").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .collect()
}

fn meta_id(e: &Record) -> Option<String> {
    Account::new(obj(e.get("oauthAccount"))?, None).map(|a| a.id)
}

/// 쓰기 길에서 읽는 목록 해석 — 깨진 꼴은 전부 None 이고 그 밖은 목록(macOS `writableSavedAccountEntries` 와 같다).
/// 없음·빈 바이트·공백만·`{}`·`accounts` 없음 → 빈 목록(빈 목록 위에 더해 쓴다), 객체 배열 → 그대로.
/// JSON 이 아님·최상위가 객체가 아님·`accounts` 가 배열이 아닌(null 포함)·객체 아닌 원소가 섞임 → 깨짐(None).
/// 깨짐은 일부만 살리지 않는다 — 일부만 살리면 다른 저장 계정이 목록에서 조용히 사라진다.
fn writable_meta(data: Option<&[u8]>) -> Option<Vec<Record>> {
    let Some(data) = data.filter(|d| !d.is_empty()) else { return Some(Vec::new()) };
    if data.iter().all(|b| matches!(*b, 0x20 | 0x09 | 0x0A | 0x0D)) {
        return Some(Vec::new());
    }
    let j: Value = serde_json::from_slice::<Value>(data).ok().filter(|j| j.is_object())?;
    match j.get("accounts") {
        // accounts 가 없으면 빈 목록 — 없는 것과 깨진 것은 다르다
        None => Some(Vec::new()),
        Some(Value::Array(list)) => {
            list.iter()
                .map(|v| match v {
                    Value::Object(m) => Some(m.clone()),
                    _ => None,
                })
                .collect()
        }
        // null·객체·수 등 배열이 아니면 깨진 것 — 일부도 살리지 않는다
        Some(_) => None,
    }
}

/// 옆으로 옮기지 못했을 때의 실패 글자 — 깨진 목록을 건드리지 않았다는 뜻이기도 하다(§plan 행 #3).
const MOVE_ASIDE_FAILED: &str = "저장 계정 목록(accounts.json)을 읽지 못해 옆으로 옮기려 했지만 옮기지 못했습니다";

/// 깨진 저장 계정 목록을 `accounts.json.broken-<지역 시각>` 으로 옆으로 옮긴다 — 같은 이름이 이미 있으면 `-2`, `-3` 을 더한다.
/// 옮긴 사실은 board.log 에 한 줄 남긴다(macOS 와 같은 글자), 실패도 한 줄 남기고 오류를 돌려준다.
fn set_aside(meta: &Path, now: f64) -> std::io::Result<()> {
    let stamp = clock::local(now).stamp();
    let mut base = meta.as_os_str().to_os_string();
    base.push(format!(".broken-{stamp}"));
    let mut target = base.clone();
    let mut n = 2;
    while Path::new(&target).exists() {
        target = {
            let mut os = base.clone();
            os.push(format!("-{n}"));
            n += 1;
            os
        };
    }
    // 옮긴 파일은 그대로 두어 사용자가 항목을 되살릴 수 있게 한다
    if let Err(e) = fs::rename(meta, Path::new(&target)) {
        board_log(&format!("저장 계정 목록(accounts.json)을 읽지 못해 옆으로 옮기려 했지만 옮기지 못함: {e}"));
        return Err(e);
    }
    let name = Path::new(&target).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    board_log(&format!("저장 계정 목록(accounts.json)을 읽지 못해 {name} 로 옮김"));
    Ok(())
}

fn write_meta(path: &Path, list: Vec<Record>) -> Result<(), String> {
    let list: Vec<Value> = list.into_iter().map(Value::Object).collect();
    let bytes = serde_json::to_vec_pretty(&json!({ "accounts": list })).map_err(|e| e.to_string())?;
    write_private(path, "accounts", &bytes).map_err(|e| e.to_string())
}

/// 저장한 계정 목록.
pub fn load_saved(p: &AccountPaths) -> Vec<Account> {
    read_meta(&p.meta)
        .iter()
        .filter_map(|e| Account::new(obj(e.get("oauthAccount"))?, num(e.get("refreshTokenExpiresAt")).map(|ms| ms / 1000.0)))
        .collect()
}

/// 계정 토큰(claudeAiOauth)은 금고에, 보일 정보는 accounts.json 에 넣는다 — 같은 계정은 제자리에서 바꾼다.
/// 목록이 깨져 있으면 옆 파일(`accounts.json.broken-<시각>`)로 옮긴 뒤 새 목록에 더한다.
pub fn store(p: &AccountPaths, oa: &Record, oauth: &Record) -> Result<Account, String> {
    let a = Account::new(oa, None).ok_or("계정 정보가 없습니다")?;
    let plain = serde_json::to_vec(&json!({ "claudeAiOauth": oauth })).map_err(|e| e.to_string())?;
    vault_write(p, &a.id, &plain)?;
    let now = clock::now();
    let mut e = Record::new();
    e.insert("oauthAccount".into(), Value::Object(oa.clone()));
    e.insert("savedAt".into(), json!(now));
    if let Some(r) = oauth.get("refreshTokenExpiresAt") {
        e.insert("refreshTokenExpiresAt".into(), r.clone());
    }
    let mut list = match writable_meta(fs::read(&p.meta).ok().as_deref()) {
        Some(list) => list,
        // 깨진 목록을 그대로 덮으면 다른 저장 계정의 표시 정보를 잃는다 — 옆 파일로 옮겨 두고 새 목록에 더한다.
        // 옮기다 실패하면 목록을 쓰지 않고 저장을 그만둔다(금고는 이미 썼다 — macOS 도 금고를 먼저 쓴다)
        None => {
            set_aside(&p.meta, now).map_err(|_| MOVE_ASIDE_FAILED.to_string())?;
            Vec::new()
        }
    };
    match list.iter().position(|x| meta_id(x).as_deref() == Some(a.id.as_str())) {
        Some(i) => list[i] = e,
        None => list.push(e),
    }
    write_meta(&p.meta, list)?;
    Ok(a)
}

/// 부분 로그인에서 토큰을 덮어 버리면 그 로그인을 잃는다 — 이 안내로 전환을 막는다(§plan 행 #1).
const CURRENT_ACCOUNT_MISSING: &str = "지금 로그인된 계정의 정보가 없어 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요";
/// oauthAccount 는 있지만 accountUuid 가 없어 저장할 수 없을 때의 안내(§plan 행 #1).
const CURRENT_ACCOUNT_INCOMPLETE: &str = "지금 로그인된 계정의 정보가 모자라 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요";

/// 두 로그인이 같은 로그인인지 — refreshToken 이 같으면 같다(전환 사이에 회전하지 않았다는 뜻).
/// 어느 쪽에 refreshToken 이 없거나 빈 글자·글자가 아니면 같다고 볼 수 없다.
fn same_login(a: &Record, b: Option<&Record>) -> bool {
    let Some(b) = b else { return false };
    match (a.get("refreshToken"), b.get("refreshToken")) {
        (Some(Value::String(x)), Some(Value::String(y))) => !x.is_empty() && x == y,
        _ => false,
    }
}

/// 지금 로그인된 계정의 최신 토큰을 저장한다 — 쓰는 동안 갱신(회전)되므로 처음 저장한 것은 이미 못 쓸 수 있다.
/// `incoming` 은 바꾸려고 하는 쪽의 로그인(전환이 아니면 없음) — 지금 계정 정보가 없거나 모자라면 토큰을
/// 덮어 버릴 때 그 로그인을 잃으므로, 바꾸려는 로그인이 지금 로그인과 같은 로그인일 때만 허용한다.
/// `.storage-write` 잠금 안에서 부른다.
pub fn save_current(p: &AccountPaths, except: Option<&str>, incoming: Option<&Record>) -> Result<(), String> {
    let oa = oauth_account(&p.claude_json);
    let cur = oa.as_ref().and_then(|oa| Account::new(oa, None));
    if cur.as_ref().is_some_and(|a| Some(a.id.as_str()) == except) {
        return Ok(());
    }
    let Some(co) = claude_ai_oauth(&p.credentials) else { return Ok(()) };
    let (Some(oa), Some(_)) = (&oa, &cur) else {
        if same_login(&co, incoming) {
            return Ok(());
        }
        return Err(if oa.is_none() { CURRENT_ACCOUNT_MISSING } else { CURRENT_ACCOUNT_INCOMPLETE }.into());
    };
    store(p, oa, &co).map(|_| ())
}

/// Claude Code 가 로그인 정보를 쓰고 있어 잠금을 못 잡았을 때의 실패 글자 — 시험 Busy 판정 기준(§plan 3.3).
pub const LOCK_BUSY: &str = "Claude Code 가 로그인 정보를 쓰는 중입니다. 잠시 뒤 다시 시도해 주세요";

/// Claude Code 와 같은 잠금 — proper-lockfile 처럼 `<파일>.lock` 폴더를 만들어 잡는다.
/// 15초 넘게 갱신되지 않은 잠금은 버려진 것으로 보고, 5초까지 기다린다.
pub fn with_file_lock<T>(file: &Path, body: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut lock = file.as_os_str().to_owned();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::create_dir(&lock) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if mtime(&lock).is_some_and(|m| clock::now() - m > 15.0) {
                    let _ = fs::remove_dir(&lock);
                    continue;
                }
                if Instant::now() >= deadline {
                    return Err(LOCK_BUSY.into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Err(format!("잠금을 만들지 못했습니다: {}", lock.display())),
        }
    }
    let r = body();
    let _ = fs::remove_dir(&lock);
    r
}

/// JSON 설정 파일의 몇 키만 바꿔 쓴다 — Claude Code 와 같은 잠금 아래에서 읽고 고친 뒤 임시 파일을 거쳐 바꿔 넣는다.
pub fn patch_json(path: &Path, change: impl FnOnce(&mut Record)) -> Result<(), String> {
    with_file_lock(path, || {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut j = fileio::read_record(path).ok_or_else(|| format!("{name} 를 읽지 못했습니다"))?;
        change(&mut j);
        let bytes = serde_json::to_vec_pretty(&Value::Object(j)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let perms = fs::metadata(path).map(|m| m.permissions()).ok();
        fileio::write_atomic(path, &name, &bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(perms) = perms {
            let _ = fs::set_permissions(path, perms);
        }
        Ok(())
    })
}

/// 전환 실패의 쪽 — 다음에 어떻게 다룰지가 달라진다(§plan 3.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwitchSide {
    /// 바꾸려던 대상 계정 쪽 탓 — 그 계정을 건너뛴다
    Target,
    /// 지금 계정·시스템 쪽 탓 — 잠시 쉬고 다시 본다
    Other,
    /// Claude Code 가 로그인 정보를 쓰고 있어 잠금을 못 잡았다 — 잠깐 기다린다
    Busy,
}

/// 전환 실패 — 실패 안내 글자와 어느 쪽 탓인지를 실어 나른다.
#[derive(Debug, Clone)]
pub struct SwitchError {
    pub message: String,
    pub side: SwitchSide,
}

impl SwitchError {
    fn new(side: SwitchSide, message: impl Into<String>) -> Self {
        SwitchError { message: message.into(), side }
    }
}

/// 전환 결과를 받는 콜백 — 성공이면 빈 값, 실패면 어느 쪽 탓인지를 돌려준다.
pub type SwitchDone = Box<dyn FnOnce(Result<(), SwitchError>) + Send>;

/// 저장한 계정으로 바꿔 넣는다. 고칠 수 없었던 부분이 있으면 그 안내를 돌려준다.
/// 실패는 `SwitchError` 로 — 어느 쪽 탓인지(`side`)와 안내 글자(`message`)를 실어 나른다.
pub fn perform_switch(p: &AccountPaths, target: &Account) -> Result<Option<String>, SwitchError> {
    let oauth = vault_read(p, &target.id)
        .and_then(|j| obj(j.get("claudeAiOauth")).cloned())
        .ok_or_else(|| SwitchError::new(SwitchSide::Target, "저장된 로그인 정보가 없습니다. 계정을 다시 추가해 주세요"))?;
    let previous = oauth_account(&p.claude_json).and_then(|oa| Account::new(&oa, None)).map(|a| a.id);
    // Claude Code 가 토큰을 갱신해 쓸 때 잡는 잠금 — 그 사이에 끼어 앞 계정 토큰으로 덮이지 않게 한다
    with_file_lock(&p.claude_dir.join(".storage-write"), || {
        let mut cur = Record::new();
        if p.credentials.exists() {
            cur = fileio::read_record(&p.credentials).ok_or("지금 로그인 정보를 읽지 못했습니다")?;
        }
        save_current(p, Some(&target.id), Some(&oauth))?;
        // 계정 토큰만 바꿔 넣는다 — MCP 서버 로그인(mcpOAuth) 같은 다른 값은 그대로 둔다
        cur.insert("claudeAiOauth".into(), Value::Object(oauth));
        let bytes = serde_json::to_vec(&Value::Object(cur)).map_err(|e| e.to_string())?;
        write_private(&p.credentials, ".credentials", &bytes).map_err(|e| format!("로그인 정보 파일에 쓰지 못했습니다: {e}"))
    })
    .map_err(|e| {
        if e == LOCK_BUSY {
            SwitchError::new(SwitchSide::Busy, e)
        } else {
            SwitchError::new(SwitchSide::Other, e)
        }
    })?;
    // 계정 정보 — /status·보드 계정 표시가 새 계정으로 보이게. 다른 계정이면 앞 계정의 캐시도 비운다
    let changed = previous.as_deref() != Some(target.id.as_str());
    let result = patch_json(&p.claude_json, |j| {
        j.insert("oauthAccount".into(), Value::Object(target.oauth.clone()));
        if changed {
            for k in ACCOUNT_CACHES {
                j.remove(k);
            }
        }
    });
    Ok(result.err().map(|e| format!("토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다: {e}")))
}

/// 금고 파일을 지우지 못해 목록에 그대로 둘 때의 실패 글자(§plan 행 #9b).
const REMOVE_VAULT_FAILED: &str = "저장한 로그인 정보를 지우지 못해 목록에 그대로 두었습니다. 잠시 뒤 다시 지워 주세요";
/// 로그인 정보는 지웠지만 목록이 깨져 목록을 건드리지 않았을 때의 경고 글자.
const REMOVE_LIST_BROKEN: &str = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 읽지 못해 목록은 그대로 두었습니다";
/// 로그인 정보는 지웠지만 목록을 쓰지 못했을 때의 경고 글자 앞부분 — 뒤에 오류를 붙인다.
const REMOVE_LIST_WRITE_FAILED: &str = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: ";

/// 저장한 계정을 지운다 — 금고 파일을 지우고, 지워졌는지 본 뒤 목록에서 뺀다. 다 했으면 `Ok(None)`.
/// 금고 파일이 지운 뒤에도 남아 있으면(없어서 못 지운 것은 지운 것으로 본다) `Err` 로 돌려주고 목록은 건드리지 않는다
/// (목록에서 빼면 다시 지울 길이 없다). 금고는 지웠는데 목록을 고치지 못했으면 `Ok(Some(안내))` —
/// 목록이 깨졌으면 목록 파일을 건드리지 않고(빈 목록으로 덮으면 다른 계정의 표시 정보를 잃는다, 깨진 파일은
/// 다음 저장·전환이 옆으로 옮긴다), 목록을 못 쓰면 그 오류를 붙인다.
pub fn remove(p: &AccountPaths, id: &str) -> Result<Option<String>, String> {
    match fs::remove_file(p.vault(id)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(REMOVE_VAULT_FAILED.into()),
    }
    if p.vault(id).exists() {
        return Err(REMOVE_VAULT_FAILED.into());
    }
    let Some(entries) = writable_meta(fs::read(&p.meta).ok().as_deref()) else { return Ok(Some(REMOVE_LIST_BROKEN.into())) };
    let kept: Vec<Record> = entries.into_iter().filter(|e| meta_id(e).as_deref() != Some(id)).collect();
    match write_meta(&p.meta, kept) {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(format!("{REMOVE_LIST_WRITE_FAILED}{e}"))),
    }
}

/// 로그인 자리를 살펴본 결과.
pub enum LoginPoll {
    Waiting,
    /// 로그인 토큰은 생겼는데 계정 정보가 아직 없다
    TokenOnly,
    Done(Result<Account, String>),
}

/// 그 설정 폴더에 계정 정보와 토큰이 생기면 저장하고(지금 계정도 함께) 자리를 치운다.
pub fn poll_login(p: &AccountPaths, slot: &Path) -> LoginPoll {
    let oauth = claude_ai_oauth(&slot.join(".credentials.json"));
    let oa = oauth_account(&slot.join(".claude.json"));
    match (oa, oauth) {
        (Some(oa), Some(oauth)) if Account::new(&oa, None).is_some() => {
            let r = store(p, &oa, &oauth).inspect(|a| {
                // 지금 계정도 함께 저장해 둔다 — 목록에 나란히 보이고 언제든 돌아올 수 있게.
                // incoming 없이 — 자리를 넘기는 중이라 지금 토큰 파일을 바꿀 일은 없다
                let _ = with_file_lock(&p.claude_dir.join(".storage-write"), || save_current(p, Some(&a.id), None));
            });
            let _ = fs::remove_dir_all(slot);
            LoginPoll::Done(r)
        }
        (_, Some(_)) => LoginPoll::TokenOnly,
        _ => LoginPoll::Waiting,
    }
}

/// 지난번에 끝나지 않은 로그인 자리를 치운다.
pub fn cleanup_slots(p: &AccountPaths) {
    for e in fs::read_dir(&p.root).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with("login-") {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

// ── 상태 ─────────────────────────────────────────────────────

#[derive(Default)]
pub struct Accounts {
    pub current: Option<Account>,
    current_mtime: Option<f64>,
    pub saved: Vec<Account>,
    pub adding: bool,
    pub switching: bool,
    /// 계정 추가 — (로그인 자리, 시작 시각)
    login: Option<(PathBuf, f64)>,
    token_only_since: Option<f64>,
    polling: bool,
    notice: Option<(Notice, f64)>,
}

impl Accounts {
    /// 계정 정보 파일이 바뀌었을 때만 다시 읽는다 — 터미널에서 /login 으로 바꿔도 따라간다.
    pub fn load_current(&mut self, p: &AccountPaths, force: bool) {
        let m = mtime(&p.claude_json);
        if !force && m == self.current_mtime {
            return;
        }
        self.current_mtime = m;
        self.current = oauth_account(&p.claude_json).and_then(|oa| Account::new(&oa, None));
    }

    pub fn load_saved(&mut self, p: &AccountPaths) {
        self.saved = load_saved(p);
    }

    /// 지금 기다리는 계정 추가 로그인의 자리 — 로그인 ConPTY(`account_login`)가 자기가 아직 필요한지 본다.
    pub fn login_slot(&self) -> Option<&Path> {
        self.login.as_ref().map(|(slot, _)| slot.as_path())
    }

    /// 안내 문구 — 7초 뒤 사라진다.
    pub fn flash(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some((Notice { text: text.into(), error }, clock::now() + 7.0));
    }

    pub fn view(&self, auto_switch: bool, now: f64) -> AccountsView {
        AccountsView {
            current: self.current.as_ref().map(|a| a.view(now)),
            saved: self.saved.iter().map(|a| a.view(now)).collect(),
            adding: self.adding,
            switching: self.switching,
            auto_switch,
            notice: self.notice.as_ref().filter(|(_, until)| *until > now).map(|(n, _)| n.clone()),
        }
    }
}

// ── 동작 (보드) ──────────────────────────────────────────────

/// 저장한 계정으로 전환한다. `note` = 전환했을 때 띄울 안내(자동 전환), `done` = 결과를 받는다.
/// 결과는 성공 여부만이 아니라 실패의 쪽(`SwitchError.side`)까지 넘긴다.
pub fn switch_to(board: &Arc<Board>, target: Account, note: Option<String>, done: Option<SwitchDone>) {
    {
        let mut acc = board.accounts.lock().unwrap();
        if acc.switching {
            drop(acc);
            if let Some(done) = done {
                done(Err(SwitchError::new(SwitchSide::Other, "이미 전환 중입니다")));
            }
            return;
        }
        acc.switching = true;
    }
    board.publish();
    let board = board.clone();
    std::thread::spawn(move || {
        let r = {
            let _io = board.account_io.lock().unwrap();
            perform_switch(&board.account_paths, &target)
        };
        let ok = r.is_ok();
        {
            let mut acc = board.accounts.lock().unwrap();
            acc.switching = false;
            acc.load_current(&board.account_paths, true);
            acc.load_saved(&board.account_paths);
            match &r {
                Ok(warning) => {
                    let error = warning.is_some();
                    let text = warning.clone().or(note).unwrap_or_else(|| {
                        format!("{} 계정으로 전환했습니다. 실행 중인 세션도 다음 요청부터 이 계정을 씁니다", target.email())
                    });
                    acc.flash(text, error);
                }
                Err(e) => {
                    // 자동 전환(note 있음)은 실패 안내를 done 에 맡긴다 — 여기서 또 알리지 않는다
                    if note.is_none() {
                        acc.flash(format!("전환하지 못했습니다: {}", e.message), true);
                    }
                }
            }
        }
        if ok {
            board.on_account_switched();
        }
        if let Some(done) = done {
            done(r.map(|_| ()));
        }
        board.publish();
    });
}

/// 계정 추가: 따로 정한 설정 폴더(CLAUDE_CONFIG_DIR)로 `claude auth login` 을 보드가 쥔 가짜 콘솔(ConPTY)에서 띄운다 — 지금 로그인은 건드리지 않는다.
/// 주소·코드·종료 처리는 `account_login` 이 맡고, 끝났는지는 아래 `tick` 이 이 폴더를 지켜본다.
/// 셸을 거치지 않고 claude.exe 를 인자 배열로 직접 띄운다(SPEC §5.7).
pub fn begin_add(board: &Arc<Board>, email: Option<String>) {
    let started = (|| {
        let mut acc = board.accounts.lock().unwrap();
        if acc.login.is_some() {
            return Ok(false);
        }
        // 시험 모드(CLAUDE_BOARD_CLAUDE)면 가짜로, 설정이 어긋났으면 진짜 claude 를 찾지도 않고 거절한다
        let (claude, mut args) = crate::test_claude::login_program(crate::test_claude::decision_from_process(), paths::overridden(), crate::terminal::find_claude)?;
        let slot = board.account_paths.root.join(format!("login-{}", short_id()));
        fs::create_dir_all(&slot).map_err(|e| format!("로그인 자리를 만들지 못했습니다: {e}"))?;
        args.extend(["auth".to_string(), "login".into(), "--claudeai".into()]);
        if let Some(e) = email.filter(|e| !e.trim().is_empty()) {
            args.push("--email".into());
            args.push(e.trim().to_string());
        }
        if let Err(e) = crate::account_login::start_for_board(board, &claude, args, &slot) {
            let _ = fs::remove_dir_all(&slot);
            return Err(format!("로그인 명령을 띄우지 못했습니다: {e}"));
        }
        acc.login = Some((slot, clock::now()));
        acc.adding = true;
        acc.flash("설정 화면의 로그인 창에서 추가할 계정으로 로그인해 주세요", false);
        Ok(true)
    })();
    if let Err(e) = started {
        board.accounts.lock().unwrap().flash(e, true);
    }
    board.publish();
}

pub fn cancel_add(board: &Arc<Board>) {
    let (slot, session) = {
        let mut acc = board.accounts.lock().unwrap();
        acc.token_only_since = None;
        acc.adding = false;
        // 계정 잠금을 쥔 채 집는다 — begin_add 도 이 잠금 안에서 새 로그인을 올리므로 다른 로그인을 집지 않는다
        (acc.login.take(), crate::account_login::current_session())
    };
    if let Some((slot, _)) = slot {
        // 로그인 프로세스를 먼저 죽이고 끝난 것을 확인한 뒤 지운다 — 죽기 전의 claude 가 지운 슬롯에 평문 토큰을 다시 쓰지 못하게
        crate::account_login::stop_session_and_remove_slot(session, &slot);
    }
    board.publish();
}

/// 3초마다 — 계정 정보 파일을 다시 읽고, 계정 추가 로그인이 끝났는지 본다.
pub fn tick(board: &Arc<Board>) {
    let now = clock::now();
    let login = {
        let mut acc = board.accounts.lock().unwrap();
        acc.load_current(&board.account_paths, false);
        match acc.login.clone() {
            Some(_) if acc.polling => None,
            Some((slot, started)) => {
                if now - started > 15.0 * 60.0 {
                    drop(acc);
                    cancel_add(board);
                    board.accounts.lock().unwrap().flash("로그인을 15분 동안 기다리다 그만뒀습니다", true);
                    return;
                }
                acc.polling = true;
                Some(slot)
            }
            None => None,
        }
    };
    let Some(slot) = login else { return };
    let board = board.clone();
    std::thread::spawn(move || {
        let result = {
            let _io = board.account_io.lock().unwrap();
            poll_login(&board.account_paths, &slot)
        };
        let mut acc = board.accounts.lock().unwrap();
        acc.polling = false;
        let same = acc.login.as_ref().is_some_and(|(s, _)| *s == slot);
        match result {
            // 프로필을 받지 못하면 Claude Code 가 계정 정보를 남기지 않는다 — 30초 넘게 그러면 다시 해 보라고 알린다
            LoginPoll::TokenOnly if same => {
                let since = *acc.token_only_since.get_or_insert(clock::now());
                if clock::now() - since > 30.0 {
                    drop(acc);
                    cancel_add(&board);
                    board.accounts.lock().unwrap().flash("로그인은 됐지만 계정 정보를 받지 못했습니다. 계정 추가를 다시 해 주세요", true);
                    board.publish();
                }
            }
            LoginPoll::Done(r) if same => {
                acc.login = None;
                acc.token_only_since = None;
                acc.adding = false;
                acc.load_saved(&board.account_paths);
                let session = crate::account_login::current_session();
                match r {
                    Ok(a) => acc.flash(format!("{} 계정을 추가했습니다. 계정 메뉴에서 골라 바로 전환할 수 있습니다", a.email()), false),
                    Err(e) => acc.flash(format!("계정을 추가하지 못했습니다: {e}"), true),
                }
                drop(acc);
                board.publish();
                // 저장을 마친 슬롯은 이미 지웠지만 명령이 아직 돌면 다시 쓸 수 있으니 죽이고 확인한 뒤 한 번 더 지운다
                crate::account_login::stop_session_and_remove_slot(session, &slot);
            }
            _ => {}
        }
    });
}

pub fn remove_account(board: &Arc<Board>, id: String) {
    let board = board.clone();
    std::thread::spawn(move || {
        let result = {
            let _io = board.account_io.lock().unwrap();
            remove(&board.account_paths, &id)
        };
        let mut acc = board.accounts.lock().unwrap();
        let email = acc.saved.iter().find(|a| a.id == id).map(Account::email);
        acc.load_saved(&board.account_paths);
        if let Some(email) = email {
            match result {
                Ok(None) => acc.flash(format!("{email} 계정을 목록에서 지웠습니다"), false),
                Ok(Some(warning)) => acc.flash(format!("{email}: {warning}"), true),
                Err(e) => acc.flash(format!("{email} 계정을 지우지 못했습니다: {e}"), true),
            }
        }
        drop(acc);
        board.publish();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        root: PathBuf,
        p: AccountPaths,
    }

    impl Fake {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("cb-accounts-{}", short_id()));
            let claude_dir = root.join(".claude");
            fs::create_dir_all(&claude_dir).unwrap();
            let p = AccountPaths {
                credentials: claude_dir.join(".credentials.json"),
                claude_json: root.join(".claude.json"),
                meta: root.join("notifier").join("accounts.json"),
                root: root.join("notifier").join("accounts"),
                claude_dir,
            };
            Fake { root, p }
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn oa(uuid: &str, email: &str) -> Record {
        json!({"accountUuid": uuid, "organizationUuid": "org", "emailAddress": email, "organizationName": "회사",
               "organizationType": "claude_max", "organizationRateLimitTier": "default_claude_max_5x"})
        .as_object()
        .unwrap()
        .clone()
    }

    fn token(t: &str) -> Record {
        json!({"accessToken": t, "refreshToken": format!("r-{t}"), "expiresAt": 1, "refreshTokenExpiresAt": 4_000_000_000_000u64})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn account_labels() {
        let a = Account::new(&oa("u1", "a@x.com"), Some(10.0)).unwrap();
        assert_eq!(a.id, "u1|org");
        assert_eq!((a.email(), a.plan()), ("a@x.com".to_string(), "Max 5x".to_string()));
        assert!(a.expired(11.0) && !a.expired(9.0));
        let pro = json!({"accountUuid": "u", "organizationType": "claude_pro"}).as_object().unwrap().clone();
        assert_eq!(Account::new(&pro, None).unwrap().plan(), "Pro");
        assert!(Account::new(&json!({"accountUuid": ""}).as_object().unwrap().clone(), None).is_none());
        // 등급만 있어도 앞 빈칸이 남지 않는다(§plan 행 #9a)
        let tier_only = json!({"accountUuid": "u", "organizationRateLimitTier": "default_claude_max_5x"}).as_object().unwrap().clone();
        assert_eq!(Account::new(&tier_only, None).unwrap().plan(), "5x");
    }

    #[test]
    fn store_switch_and_remove() {
        let f = Fake::new();
        let p = &f.p;
        // 지금 로그인 = a, 다른 키(mcpOAuth)·계정별 캐시가 있다
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A1"), "mcpOAuth": {"x": 1}})).unwrap()).unwrap();
        fs::write(&p.claude_json, serde_json::to_vec(&json!({"numStartups": 3, "oauthAccount": oa("ua", "a@x.com"), "modelAccessCache": {"m": 1}})).unwrap()).unwrap();
        // b 를 저장해 둔다
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        assert_eq!(load_saved(p).len(), 1);
        assert_eq!(load_saved(p)[0].refresh_expires, Some(4_000_000_000.0));
        // 금고 파일은 계정 id 해시 이름
        assert!(p.vault(&b.id).exists());
        assert_eq!(p.vault(&b.id).file_name().unwrap().len(), 16 + 4);
        // 금고 파일 내용 — Windows 는 DPAPI 로 봉인돼 토큰이 평문으로 보이지 않는다(그 밖은 개발용 평문)
        let raw = fs::read(p.vault(&b.id)).unwrap();
        for needle in ["claudeAiOauth", "accessToken", "r-B1"] {
            assert_eq!(
                crate::util::contains(&raw, needle.as_bytes()),
                !cfg!(windows),
                "금고 파일({}바이트)에서 {needle:?} 평문 여부가 기대와 다르다: {}",
                raw.len(),
                String::from_utf8_lossy(&raw[..raw.len().min(200)])
            );
        }

        // 지금 계정의 토큰이 회전된 뒤 b 로 전환 — a 의 최신 토큰을 저장하고 b 토큰을 넣는다
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A2"), "mcpOAuth": {"x": 1}})).unwrap()).unwrap();
        assert_eq!(perform_switch(p, &b).unwrap(), None);
        let cred = fileio::read_record(&p.credentials).unwrap();
        assert_eq!(cred["claudeAiOauth"]["accessToken"], "B1");
        assert_eq!(cred["mcpOAuth"]["x"], 1);
        let cj = fileio::read_record(&p.claude_json).unwrap();
        assert_eq!(cj["oauthAccount"]["emailAddress"], "b@x.com");
        assert_eq!(cj["numStartups"], 3);
        assert!(cj.get("modelAccessCache").is_none());
        assert!(!p.claude_dir.join(".storage-write.lock").exists());
        let saved = load_saved(p);
        assert_eq!(saved.iter().map(Account::email).collect::<Vec<_>>(), ["b@x.com", "a@x.com"]);
        // a 로 되돌아오면 회전된 A2 를 쓴다
        let a = saved.iter().find(|x| x.email() == "a@x.com").unwrap().clone();
        perform_switch(p, &a).unwrap();
        assert_eq!(fileio::read_record(&p.credentials).unwrap()["claudeAiOauth"]["accessToken"], "A2");

        // 금고가 없는 계정은 전환하지 않는다
        let ghost = Account::new(&oa("ghost", "g@x.com"), None).unwrap();
        assert!(perform_switch(p, &ghost).unwrap_err().message.contains("저장된 로그인 정보가 없습니다"));

        assert_eq!(remove(p, &b.id), Ok(None));
        assert!(!p.vault(&b.id).exists());
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["a@x.com"]);
    }

    #[test]
    fn switch_error_target_side() {
        let f = Fake::new();
        let p = &f.p;
        // 금고가 없는 계정은 대상 계정 쪽 실패다
        let ghost = Account::new(&oa("ghost", "g@x.com"), None).unwrap();
        let e = perform_switch(p, &ghost).unwrap_err();
        assert_eq!(e.side, SwitchSide::Target);
        assert!(e.message.contains("저장된 로그인 정보가 없습니다"));
    }

    #[test]
    fn switch_error_other_side() {
        let f = Fake::new();
        let p = &f.p;
        // 지금 로그인 정보 파일이 깨졌으면 그 밖(지금 계정·시스템) 쪽 실패다
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        fs::write(&p.credentials, b"{oops").unwrap();
        let e = perform_switch(p, &b).unwrap_err();
        assert_eq!(e.side, SwitchSide::Other);
        assert!(e.message.contains("지금 로그인 정보를 읽지 못했습니다"));
    }

    #[test]
    fn switch_error_busy_side() {
        let f = Fake::new();
        let p = &f.p;
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        // Claude Code 가 로그인 정보를 쓰고 있으면(잠금 시간 초과) 바쁨이다
        fs::create_dir(p.claude_dir.join(".storage-write.lock")).unwrap();
        let e = perform_switch(p, &b).unwrap_err();
        assert_eq!(e.side, SwitchSide::Busy);
        assert_eq!(e.message, LOCK_BUSY);
    }

    #[test]
    fn locks_and_patch() {
        let f = Fake::new();
        let file = f.p.claude_dir.join(".storage-write");
        // 잡힌 잠금은 5초 기다리다 실패한다
        fs::create_dir(f.p.claude_dir.join(".storage-write.lock")).unwrap();
        let t = Instant::now();
        assert!(with_file_lock(&file, || Ok(())).unwrap_err().contains("쓰는 중"));
        assert!(t.elapsed() >= Duration::from_secs(4));
        fs::remove_dir(f.p.claude_dir.join(".storage-write.lock")).unwrap();
        assert_eq!(with_file_lock(&file, || Ok(7)).unwrap(), 7);
        // 깨진 파일은 고치지 않는다
        fs::write(&f.p.claude_json, b"{oops").unwrap();
        assert!(patch_json(&f.p.claude_json, |_| {}).unwrap_err().contains(".claude.json"));
        assert_eq!(fs::read(&f.p.claude_json).unwrap(), b"{oops");
    }

    #[test]
    fn login_polling() {
        let f = Fake::new();
        let p = &f.p;
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A1")})).unwrap()).unwrap();
        fs::write(&p.claude_json, serde_json::to_vec(&json!({"oauthAccount": oa("ua", "a@x.com")})).unwrap()).unwrap();
        let slot = p.root.join("login-abcd1234");
        fs::create_dir_all(&slot).unwrap();
        assert!(matches!(poll_login(p, &slot), LoginPoll::Waiting));
        fs::write(slot.join(".credentials.json"), serde_json::to_vec(&json!({"claudeAiOauth": token("N1")})).unwrap()).unwrap();
        assert!(matches!(poll_login(p, &slot), LoginPoll::TokenOnly));
        fs::write(slot.join(".claude.json"), serde_json::to_vec(&json!({"oauthAccount": oa("un", "n@x.com")})).unwrap()).unwrap();
        let LoginPoll::Done(Ok(a)) = poll_login(p, &slot) else { panic!() };
        assert_eq!(a.email(), "n@x.com");
        assert!(!slot.exists());
        // 새 계정과 지금 계정이 함께 저장된다
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["n@x.com", "a@x.com"]);
        fs::create_dir_all(p.root.join("login-old")).unwrap();
        cleanup_slots(p);
        assert!(!p.root.join("login-old").exists() && p.root.join("vault").exists());
    }

    // ── 지금 계정을 먼저 저장할 수 없으면 전환을 막는다(§plan 행 #1) ──

    /// 지금 계정 정보(oauthAccount)가 없거나 모자란 상태로 토큰 파일을 바꾸면 그 로그인을 잃는다.
    /// 토큰 없음 → 넘어감, 없음·모자람 + 토큰 → 막음, 같은 refreshToken → 막지 않음,
    /// 다름·한쪽 없음·빈 글자·글자 아님 → 막음.
    #[test]
    fn save_current_blocks_without_account_info() {
        let f = Fake::new();
        let p = &f.p;
        // 토큰이 없으면 막지 않는다 — 저장할 것도 없다
        fs::write(&p.claude_json, b"{}").unwrap();
        fs::write(&p.credentials, b"{}").unwrap();
        assert!(save_current(p, None, None).is_ok());
        assert!(load_saved(p).is_empty());
        // 계정 정보 없이 토큰이 있으면 그 토큰을 못 잃게 막는다(없음)
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A1")})).unwrap()).unwrap();
        assert_eq!(save_current(p, None, None).unwrap_err(), CURRENT_ACCOUNT_MISSING);
        // 바꾸려는 로그인과 지금 로그인이 다르면 막는다
        assert_eq!(save_current(p, None, Some(&token("B1"))).unwrap_err(), CURRENT_ACCOUNT_MISSING);
        // 같은 로그인이면 막지 않는다 — 목록에는 아무것도 더하지 않는다(전환 중이다)
        assert!(save_current(p, None, Some(&token("A1"))).is_ok());
        assert!(load_saved(p).is_empty());
        // refreshToken 이 없거나 빈 글자·글자가 아니면 같은 로그인이라고 볼 수 없다 → 막음
        let mut no_refresh = token("A1");
        no_refresh.remove("refreshToken");
        assert!(save_current(p, None, Some(&no_refresh)).is_err());
        let mut empty = token("A1");
        empty.insert("refreshToken".into(), json!(""));
        assert!(save_current(p, None, Some(&empty)).is_err());
        let mut number = token("A1");
        number.insert("refreshToken".into(), json!(5));
        assert!(save_current(p, None, Some(&number)).is_err());
        // 계정 정보가 모자라다(accountUuid 없음) → 막음(모자람)
        fs::write(&p.claude_json, serde_json::to_vec(&json!({"oauthAccount": {"emailAddress": "a@x.com"}})).unwrap()).unwrap();
        assert_eq!(save_current(p, None, None).unwrap_err(), CURRENT_ACCOUNT_INCOMPLETE);
        // 같은 로그인이면 모자란 상태에서도 계속한다
        assert!(save_current(p, None, Some(&token("A1"))).is_ok());
        // except 가 지금 계정과 같으면 같은 로그인 검사 전에 건너뛴다
        fs::write(&p.claude_json, serde_json::to_vec(&json!({"oauthAccount": oa("ua", "a@x.com"), "k": 1})).unwrap()).unwrap();
        assert!(save_current(p, Some("ua|org"), Some(&token("B1"))).is_ok());
        assert!(load_saved(p).is_empty());
        // 지금 계정 정보가 온전하면 종전처럼 저장한다
        assert!(save_current(p, None, Some(&token("B1"))).is_ok());
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["a@x.com"]);
    }

    /// 전환이 막히면 어느 파일도 바꾸지 않는다 — 다시 시도해 볼 수 있어야 한다.
    /// 지금 로그인이 이미 바꾸려는 계정과 같은 로그인이면(로그인 슬롯이 텅 비어도) 그대로 전환한다.
    #[test]
    fn blocked_switch_writes_nothing() {
        let f = Fake::new();
        let p = &f.p;
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A2"), "mcpOAuth": {"x": 1}})).unwrap()).unwrap();
        // 지금 계정 정보가 없다(부분 로그인) — 전환을 막는다
        fs::write(&p.claude_json, b"{\"numStartups\": 3}").unwrap();
        let cred = fs::read(&p.credentials).unwrap();
        let cj = fs::read(&p.claude_json).unwrap();
        let meta = fs::read(&p.meta).unwrap();
        let vault_b = fs::read(p.vault(&b.id)).unwrap();
        let e = perform_switch(p, &b).unwrap_err();
        assert_eq!(e.side, SwitchSide::Other);
        assert_eq!(e.message, CURRENT_ACCOUNT_MISSING);
        assert_eq!(fs::read(&p.credentials).unwrap(), cred);
        assert_eq!(fs::read(&p.claude_json).unwrap(), cj);
        assert_eq!(fs::read(&p.meta).unwrap(), meta);
        assert_eq!(fs::read(p.vault(&b.id)).unwrap(), vault_b);
        // 지금 로그인이 이미 b 와 같은 로그인(refreshToken r-B1)이면 그대로 전환한다
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("B1")})).unwrap()).unwrap();
        assert!(perform_switch(p, &b).is_ok());
    }

    // ── 요금제 글자(§plan 행 #9a) ──

    /// 요금제 글자 — 빈 조각을 건너뛰고 이어 붙인다: 등급만 → "5x", 둘 다 → "Max 5x", 종류만 → "Pro"(§plan 행 #9a).
    #[test]
    fn plan_joins_only_non_empty_parts() {
        let plan = |t: &str, tier: &str| {
            Account::new(
                &json!({"accountUuid": "u", "organizationType": t, "organizationRateLimitTier": tier})
                    .as_object()
                    .unwrap()
                    .clone(),
                None,
            )
            .unwrap()
            .plan()
        };
        // 등급만 있으면 앞 빈칸이 남지 않는다
        assert_eq!(plan("", "default_claude_max_5x"), "5x");
        // 둘 다 있으면 이어 붙인다
        assert_eq!(plan("claude_max", "default_claude_max_5x"), "Max 5x");
        // 종류만 있으면 종류만
        assert_eq!(plan("claude_pro", ""), "Pro");
        // 없으면 아무것도 남지 않는다
        assert_eq!(plan("", ""), "");
        // 등급 꼴이 아니면 건너뛴다 — 숫자가 아닌 자리·한 글자·x 로 안 끝나는 자리·x 만 있는 자리
        assert_eq!(plan("claude_max", "default_claude_max_five"), "Max");
        assert_eq!(plan("claude_max", "default_claude_max_5"), "Max");
        assert_eq!(plan("claude_max", "default_claude_max_x"), "Max");
        // 마지막 자리만 등급으로 보고 그 앞은 아무 꼴이나 된다
        assert_eq!(plan("claude_max", "default_claude_max_1_5x"), "Max 5x");
        assert_eq!(plan("claude_max", "_5x"), "Max 5x");
    }

    // ── 깨진 저장 계정 목록(§plan 행 #3) ──

    /// 환경변수는 프로세스 전체가 하나라, 바꾸는 시험이 동시에 둘 돌면 서로의 폴더를 덮어쓴다. 쓰는 동안 이 잠금을 쥐어 한 번에 하나만 돈다.
    static NOTIFIER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// board.log 등 전역 notifier 폴더를 시험용 폴더로 바꿨다가 되돌린다. 살아 있는 동안 `NOTIFIER_ENV_LOCK` 을 쥔다.
    struct NotifierEnv {
        old: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl NotifierEnv {
        fn for_test(dir: &Path) -> Self {
            // 앞 시험이 panic 해 잠금이 오염돼도 환경은 Drop 이 되돌렸으니 이어서 쓴다
            let guard = NOTIFIER_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let old = std::env::var_os(paths::NOTIFIER_ENV);
            std::env::set_var(paths::NOTIFIER_ENV, dir);
            NotifierEnv { old, _guard: guard }
        }
    }

    impl Drop for NotifierEnv {
        fn drop(&mut self) {
            match &self.old {
                Some(v) => std::env::set_var(paths::NOTIFIER_ENV, v),
                None => std::env::remove_var(paths::NOTIFIER_ENV),
            }
        }
    }

    /// 옆으로 옮겨진 깨진 목록 파일들 — `*.broken-*` 이름, 이름 차순.
    fn broken_asides(p: &AccountPaths) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(p.meta.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|path| path.file_name().is_some_and(|n| n.to_string_lossy().contains(".broken-")))
            .collect();
        found.sort();
        found
    }

    /// 시험용 실패 주입 — 파일을 읽기 전용 공유(Windows)로 열거나 폴더를 읽기 전용(UNIX)으로 바꿔
    /// 그 파일의 옮기기(rename)를 막는다. 바꾸기(write_atomic 의 replace)도 같은 이유로 실패한다.
    struct BlockedRename {
        _file: fs::File,
        #[cfg(unix)]
        dir: PathBuf,
    }

    impl BlockedRename {
        fn new(path: &Path) -> Self {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let dir = path.parent().unwrap().to_path_buf();
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
                BlockedRename { _file: fs::File::open(path).unwrap(), dir }
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                let file = fs::OpenOptions::new().read(true).share_mode(1).open(path).unwrap();
                BlockedRename { _file: file }
            }
        }
    }

    impl Drop for BlockedRename {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o755));
            }
        }
    }

    /// 쓰기 길에서만 쓰는 목록 해석 — 해석 표. 깨진 꼴은 전부 None 이고 일부만 살리지 않는다.
    #[test]
    fn writable_meta_parse_table() {
        let w = |b: &[u8]| writable_meta(Some(b));
        let list = |items: &[&str]| items.iter().map(|s| serde_json::from_str::<Record>(s).unwrap()).collect::<Vec<_>>();
        // 없음·빈 바이트·공백만·{}·accounts 없음 → 빈 목록(빈 목록 위에 더해 쓴다)
        assert_eq!(writable_meta(None), Some(vec![]));
        assert_eq!(w(b""), Some(vec![]));
        assert_eq!(w(b" \t\r\n"), Some(vec![]));
        assert_eq!(w(b"{}"), Some(vec![]));
        assert_eq!(w(br#"{"accounts": []}"#), Some(vec![]));
        // 객체 배열 → 그대로
        assert_eq!(w(br#"{"accounts": [{"a": 1}, {"b": 2}], "x": 3}"#), Some(list(&[r#"{"a":1}"#, r#"{"b":2}"#])));
        // JSON 이 아님·최상위가 객체가 아님(배열·수)·UTF-8 이 아닌 바이트 → 깨짐
        assert_eq!(w(b"{oops"), None);
        assert_eq!(w(b"[1]"), None);
        assert_eq!(w(b"5"), None);
        assert_eq!(w(b"\xff\xfe{junk}"), None);
        // accounts 가 배열이 아니면(null 포함) → 깨짐
        assert_eq!(w(br#"{"accounts": null}"#), None);
        assert_eq!(w(br#"{"accounts": {}}"#), None);
        assert_eq!(w(br#"{"accounts": 3}"#), None);
        // 객체 아닌 원소가 섞이면 일부도 살리지 않는다 → 깨짐
        assert_eq!(w(br#"{"accounts": [1, {"a": 1}]}"#), None);
        assert_eq!(w(br#"{"accounts": [{"a": 1}, null]}"#), None);
        assert_eq!(w(br#"{"accounts": [{"a": 1}, "x"]}"#), None);
    }

    /// 깨진 목록을 만난 저장은 옆 파일로 옮긴 뒤 새 목록에 더하고 board.log 에 한 줄 남긴다.
    #[test]
    fn store_sets_broken_meta_aside() {
        let f = Fake::new();
        let p = &f.p;
        let dir = p.meta.parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let _log = NotifierEnv::for_test(&dir);
        fs::write(&p.meta, b"{oops").unwrap();
        let _ = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
        // 새 목록은 새 항목 하나 — 옆 파일을 지우면 다른 계정 항목을 되살릴 수 있다는 뜻이기도 하다
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["a@x.com"]);
        // 깨진 파일은 broken 이름으로 옆으로 옮겨진다 — 바이트 그대로
        let kept = broken_asides(p);
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read(&kept[0]).unwrap(), b"{oops");
        let log = fs::read_to_string(dir.join("board.log")).unwrap();
        assert!(log.contains("옮김"), "board.log: {log}");
    }

    /// 옮길 이름이 이미 있으면 `-2` 를 더한다 — 고정 시각으로 두 번 옮겨 확인한다(실제 시각에 의존하지 않게).
    #[test]
    fn set_aside_suffixes_when_name_taken() {
        let f = Fake::new();
        let dir = f.p.meta.parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let _log = NotifierEnv::for_test(&dir);
        let base = dir.join(format!("accounts.json.broken-{}", clock::local(0.0).stamp()));
        fs::write(&f.p.meta, b"{first").unwrap();
        set_aside(&f.p.meta, 0.0).unwrap();
        assert_eq!(fs::read(&base).unwrap(), b"{first");
        fs::write(&f.p.meta, b"{second").unwrap();
        set_aside(&f.p.meta, 0.0).unwrap();
        let mut second = base.clone().into_os_string();
        second.push("-2");
        let second = PathBuf::from(second);
        assert_eq!(fs::read(&second).unwrap(), b"{second");
        let log = fs::read_to_string(dir.join("board.log")).unwrap();
        assert_eq!(log.lines().filter(|l| l.contains("옮김")).count(), 2);
    }

    /// 옆으로 옮기지도 못하면 저장도 그만두고 목록 파일은 그대로 둔다 — 깨진 파일에 새 항목을 덮지 않는다.
    #[test]
    fn store_fails_when_broken_file_cannot_move() {
        let f = Fake::new();
        let p = &f.p;
        let dir = p.meta.parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let _log = NotifierEnv::for_test(&dir);
        fs::write(&p.meta, b"{oops").unwrap();
        let before = fs::read(&p.meta).unwrap();
        let _lock = BlockedRename::new(&p.meta);
        let err = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap_err();
        assert_eq!(err, MOVE_ASIDE_FAILED);
        assert_eq!(fs::read(&p.meta).unwrap(), before);
        assert!(broken_asides(p).is_empty());
        let log = fs::read_to_string(dir.join("board.log")).unwrap();
        assert!(log.contains("옮기지 못함"), "board.log: {log}");
    }

    /// 목록이 깨진 상태에서도 전환은 막지 않는다 — 옆으로 옮기고 지금 계정을 새 목록에 저장한다.
    #[test]
    fn switch_still_works_with_broken_meta() {
        let f = Fake::new();
        let p = &f.p;
        let _log = NotifierEnv::for_test(p.meta.parent().unwrap());
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        // b 를 저장한 뒤에 목록이 깨졌다
        fs::write(&p.meta, b"{oops").unwrap();
        fs::write(&p.credentials, serde_json::to_vec(&json!({"claudeAiOauth": token("A2")})).unwrap()).unwrap();
        fs::write(&p.claude_json, serde_json::to_vec(&json!({"oauthAccount": oa("ua", "a@x.com")})).unwrap()).unwrap();
        assert!(perform_switch(p, &b).is_ok());
        // 옆 파일 하나 + 새 목록은 지금 계정 하나
        let kept = broken_asides(p);
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read(&kept[0]).unwrap(), b"{oops");
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["a@x.com"]);
    }

    /// 깨진 꼴마다 옆 파일 하나가 생기고 새 목록은 새 항목 하나다.
    #[test]
    fn store_sets_aside_every_broken_shape() {
        let shapes: [&[u8]; 7] = [b"{oops", b"[1]", b"\xff\xfe{junk}", br#"{"accounts": null}"#, br#"{"accounts": {}}"#, br#"{"accounts": [1, {"a": 1}]}"#, b"5"];
        for raw in shapes {
            let f = Fake::new();
            let p = &f.p;
            let dir = p.meta.parent().unwrap().to_path_buf();
            fs::create_dir_all(&dir).unwrap();
            let _log = NotifierEnv::for_test(&dir);
            fs::write(&p.meta, raw).unwrap();
            store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
            let kept = broken_asides(p);
            assert_eq!(kept.len(), 1, "{}", String::from_utf8_lossy(raw));
            assert_eq!(fs::read(&kept[0]).unwrap(), raw);
            assert_eq!(load_saved(p).len(), 1);
        }
    }

    // ── 지우기 결과(§plan 행 #9b) ──

    /// 정상으로 지우면 `Ok(None)`, 금고에 없는 계정도 지운 것으로 본다(목록에서는 빠진다).
    #[test]
    fn remove_ok_and_missing_vault() {
        let f = Fake::new();
        let p = &f.p;
        let a = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
        let b = store(p, &oa("ub", "b@x.com"), &token("B1")).unwrap();
        // 금고 파일이 이미 없어도 목록에서 뺀다
        fs::remove_file(p.vault(&a.id)).unwrap();
        assert_eq!(remove(p, &a.id), Ok(None));
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["b@x.com"]);
        assert!(p.vault(&b.id).exists());
        // 목록에 없는 계정을 지워도 아무 일 없이 끝난다
        let ghost = Account::new(&oa("ghost", "g@x.com"), None).unwrap();
        assert_eq!(remove(p, &ghost.id), Ok(None));
        assert_eq!(load_saved(p).len(), 1);
    }

    /// 금고 파일을 지우지 못하면 `Err` 이고 목록은 그대로다(목록에서 빼면 다시 지울 길이 없다).
    #[test]
    fn remove_keeps_list_when_vault_cannot_be_deleted() {
        let f = Fake::new();
        let p = &f.p;
        let a = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
        // 금고 자리를 비지 않은 폴더로 바꿔 파일 지우기를 NotFound 가 아닌 오류로 막는다
        fs::remove_file(p.vault(&a.id)).unwrap();
        fs::create_dir_all(p.vault(&a.id)).unwrap();
        fs::write(p.vault(&a.id).join("x"), b"x").unwrap();
        let meta = fs::read(&p.meta).unwrap();
        assert_eq!(remove(p, &a.id), Err("저장한 로그인 정보를 지우지 못해 목록에 그대로 두었습니다. 잠시 뒤 다시 지워 주세요".to_string()));
        assert_eq!(fs::read(&p.meta).unwrap(), meta);
    }

    /// 목록이 깨졌으면 금고만 지우고 경고를 돌려주며 깨진 파일의 바이트는 그대로다.
    #[test]
    fn remove_leaves_broken_list_untouched() {
        let f = Fake::new();
        let p = &f.p;
        let dir = p.meta.parent().unwrap().to_path_buf();
        let _log = NotifierEnv::for_test(&dir);
        let a = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
        fs::write(&p.meta, b"{oops").unwrap();
        let w = remove(p, &a.id).unwrap().unwrap();
        assert_eq!(w, "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 읽지 못해 목록은 그대로 두었습니다");
        assert!(!p.vault(&a.id).exists());
        assert_eq!(fs::read(&p.meta).unwrap(), b"{oops");
        assert!(broken_asides(p).is_empty());
    }

    /// 목록을 쓰지 못하면 경고에 오류를 붙여 돌려준다.
    #[test]
    fn remove_reports_list_write_failure() {
        let f = Fake::new();
        let p = &f.p;
        let a = store(p, &oa("ua", "a@x.com"), &token("A1")).unwrap();
        let _lock = BlockedRename::new(&p.meta);
        let w = remove(p, &a.id).unwrap().unwrap();
        assert!(w.starts_with("로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: "), "{w}");
        assert!(w.len() > "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: ".len());
        assert!(!p.vault(&a.id).exists());
    }
}
