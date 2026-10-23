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
use crate::util::{mtime, num, obj, short_id};

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

    /// 요금제 — organizationType claude_max + 한도 등급 …_5x → "Max 5x"
    pub fn plan(&self) -> String {
        let t = self.str("organizationType").replace("claude_", "");
        let mut chars = t.chars();
        let mut p: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
        let tier = self.str("organizationRateLimitTier");
        if let Some(i) = tier.rfind('_') {
            let n = &tier[i + 1..];
            if n.len() > 1 && n.ends_with('x') && n[..n.len() - 1].bytes().all(|c| c.is_ascii_digit()) {
                p.push(' ');
                p.push_str(n);
            }
        }
        p
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
    write_private(&p.vault(id), "vault", &sealed).map_err(|e| format!("로그인 정보를 저장하지 못했습니다 — {e}"))
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
pub fn store(p: &AccountPaths, oa: &Record, oauth: &Record) -> Result<Account, String> {
    let a = Account::new(oa, None).ok_or("계정 정보가 없습니다")?;
    let plain = serde_json::to_vec(&json!({ "claudeAiOauth": oauth })).map_err(|e| e.to_string())?;
    vault_write(p, &a.id, &plain)?;
    let mut e = Record::new();
    e.insert("oauthAccount".into(), Value::Object(oa.clone()));
    e.insert("savedAt".into(), json!(clock::now()));
    if let Some(r) = oauth.get("refreshTokenExpiresAt") {
        e.insert("refreshTokenExpiresAt".into(), r.clone());
    }
    let mut list = read_meta(&p.meta);
    match list.iter().position(|x| meta_id(x).as_deref() == Some(a.id.as_str())) {
        Some(i) => list[i] = e,
        None => list.push(e),
    }
    write_meta(&p.meta, list)?;
    Ok(a)
}

/// 지금 로그인된 계정의 최신 토큰을 저장한다 — 쓰는 동안 갱신(회전)되므로 처음 저장한 것은 이미 못 쓸 수 있다.
/// `.storage-write` 잠금 안에서 부른다.
pub fn save_current(p: &AccountPaths, except: Option<&str>) -> Result<(), String> {
    let Some(oa) = oauth_account(&p.claude_json) else { return Ok(()) };
    let Some(a) = Account::new(&oa, None) else { return Ok(()) };
    if Some(a.id.as_str()) == except {
        return Ok(());
    }
    let Some(co) = claude_ai_oauth(&p.credentials) else { return Ok(()) };
    store(p, &oa, &co).map(|_| ())
}

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
                    return Err("Claude Code 가 로그인 정보를 쓰는 중입니다 — 잠시 뒤 다시 시도해 주세요".into());
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

/// 저장한 계정으로 바꿔 넣는다. 고칠 수 없었던 부분이 있으면 그 안내를 돌려준다.
pub fn perform_switch(p: &AccountPaths, target: &Account) -> Result<Option<String>, String> {
    let oauth = vault_read(p, &target.id)
        .and_then(|j| obj(j.get("claudeAiOauth")).cloned())
        .ok_or("저장된 로그인 정보가 없습니다 — 계정을 다시 추가해 주세요")?;
    let previous = oauth_account(&p.claude_json).and_then(|oa| Account::new(&oa, None)).map(|a| a.id);
    // Claude Code 가 토큰을 갱신해 쓸 때 잡는 잠금 — 그 사이에 끼어 앞 계정 토큰으로 덮이지 않게 한다
    with_file_lock(&p.claude_dir.join(".storage-write"), || {
        let mut cur = Record::new();
        if p.credentials.exists() {
            cur = fileio::read_record(&p.credentials).ok_or("지금 로그인 정보를 읽지 못했습니다")?;
        }
        save_current(p, Some(&target.id))?;
        // 계정 토큰만 바꿔 넣는다 — MCP 서버 로그인(mcpOAuth) 같은 다른 값은 그대로 둔다
        cur.insert("claudeAiOauth".into(), Value::Object(oauth));
        let bytes = serde_json::to_vec(&Value::Object(cur)).map_err(|e| e.to_string())?;
        write_private(&p.credentials, ".credentials", &bytes).map_err(|e| format!("로그인 정보 파일에 쓰지 못했습니다 — {e}"))
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
    Ok(result.err().map(|e| format!("토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다 — {e}")))
}

/// 저장한 계정을 지운다.
pub fn remove(p: &AccountPaths, id: &str) {
    let _ = fs::remove_file(p.vault(id));
    let _ = write_meta(&p.meta, read_meta(&p.meta).into_iter().filter(|e| meta_id(e).as_deref() != Some(id)).collect());
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
                // 지금 계정도 함께 저장해 둔다 — 목록에 나란히 보이고 언제든 돌아올 수 있게
                let _ = with_file_lock(&p.claude_dir.join(".storage-write"), || save_current(p, Some(&a.id)));
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
pub fn switch_to(board: &Arc<Board>, target: Account, note: Option<String>, done: Option<Box<dyn FnOnce(bool) + Send>>) {
    {
        let mut acc = board.accounts.lock().unwrap();
        if acc.switching {
            drop(acc);
            if let Some(done) = done {
                done(false);
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
            match r {
                Ok(warning) => {
                    let error = warning.is_some();
                    let text = warning.or(note).unwrap_or_else(|| {
                        format!("{} 계정으로 전환했습니다 — 실행 중인 세션도 다음 요청부터 이 계정을 씁니다", target.email())
                    });
                    acc.flash(text, error);
                }
                Err(e) => acc.flash(format!("전환하지 못했습니다 — {e}"), true),
            }
        }
        if ok {
            board.on_account_switched();
        }
        if let Some(done) = done {
            done(ok);
        }
        board.publish();
    });
}

/// 계정 추가: 따로 정한 설정 폴더(CLAUDE_CONFIG_DIR)로 `claude auth login` 을 새 콘솔 창에서 띄운다 — 지금 로그인은 건드리지 않는다.
/// 셸을 거치지 않고 claude.exe 를 인자 배열로 직접 띄운다(SPEC §5.7).
pub fn begin_add(board: &Arc<Board>, email: Option<String>) {
    let started = (|| {
        let mut acc = board.accounts.lock().unwrap();
        if acc.login.is_some() {
            return Ok(false);
        }
        let claude = crate::terminal::find_claude().ok_or("claude 명령을 찾지 못했습니다")?;
        let slot = board.account_paths.root.join(format!("login-{}", short_id()));
        fs::create_dir_all(&slot).map_err(|e| format!("로그인 자리를 만들지 못했습니다 — {e}"))?;
        let mut args = vec!["auth".to_string(), "login".into(), "--claudeai".into()];
        if let Some(e) = email.filter(|e| !e.trim().is_empty()) {
            args.push("--email".into());
            args.push(e.trim().to_string());
        }
        if let Err(e) = crate::terminal::spawn_console(&claude, &args, &paths::home(), &[("CLAUDE_CONFIG_DIR", slot.as_os_str())]) {
            let _ = fs::remove_dir_all(&slot);
            return Err(format!("로그인 창을 띄우지 못했습니다 — {e}"));
        }
        acc.login = Some((slot, clock::now()));
        acc.adding = true;
        acc.flash("터미널과 브라우저에서 추가할 계정으로 로그인해 주세요", false);
        Ok(true)
    })();
    if let Err(e) = started {
        board.accounts.lock().unwrap().flash(e, true);
    }
    board.publish();
}

pub fn cancel_add(board: &Arc<Board>) {
    let slot = {
        let mut acc = board.accounts.lock().unwrap();
        acc.token_only_since = None;
        acc.adding = false;
        acc.login.take()
    };
    if let Some((slot, _)) = slot {
        let _ = fs::remove_dir_all(slot);
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
                    board.accounts.lock().unwrap().flash("로그인은 됐지만 계정 정보를 받지 못했습니다 — 계정 추가를 다시 해 주세요", true);
                    board.publish();
                }
            }
            LoginPoll::Done(r) if same => {
                acc.login = None;
                acc.token_only_since = None;
                acc.adding = false;
                acc.load_saved(&board.account_paths);
                match r {
                    Ok(a) => acc.flash(format!("{} 계정을 추가했습니다 — 계정 메뉴에서 골라 바로 전환할 수 있습니다", a.email()), false),
                    Err(e) => acc.flash(format!("계정을 추가하지 못했습니다 — {e}"), true),
                }
                drop(acc);
                board.publish();
            }
            _ => {}
        }
    });
}

pub fn remove_account(board: &Arc<Board>, id: String) {
    let board = board.clone();
    std::thread::spawn(move || {
        {
            let _io = board.account_io.lock().unwrap();
            remove(&board.account_paths, &id);
        }
        let mut acc = board.accounts.lock().unwrap();
        let email = acc.saved.iter().find(|a| a.id == id).map(Account::email);
        acc.load_saved(&board.account_paths);
        if let Some(email) = email {
            acc.flash(format!("{email} 계정을 목록에서 지웠습니다"), false);
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
        assert!(perform_switch(p, &ghost).unwrap_err().contains("저장된 로그인 정보가 없습니다"));

        remove(p, &b.id);
        assert!(!p.vault(&b.id).exists());
        assert_eq!(load_saved(p).iter().map(Account::email).collect::<Vec<_>>(), ["a@x.com"]);
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
}
