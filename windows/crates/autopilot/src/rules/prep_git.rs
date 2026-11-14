//! 출발 준비·지난 실행 이어 가기에서 보드가 git 을 읽기 전의 git 자리 확인. 원본: `Sources/AutopilotRules/PrepGit.swift`.
//! 원본 머리 주석의 요지: 고른 경로가 모델이 쓸 수 있던 작업 트리 안의 중첩 git 이면 그 git 폴더 설정의 clean filter 를 보드의 git 이
//! 샌드박스 밖에서 돌린다. 그래서 git 을 부르기 전에 파일 시스템으로만 저장소 뿌리·git 폴더를 찾고(.git 바로 가기·HEAD 가 보통 파일이 아닌 꼴·
//! gitdir/commondir 되짚기), 중첩 git·임시 폴더·떨어진 git 폴더를 막고, git 설정(core.fsmonitor·filter·include)과 하위 모듈을 본다.
//! 파일 시스템·git 은 포트(`PrepGitWorld`)로만 본다 — 진짜 어댑터는 `adapters/prep_git_world.rs`, 가짜는 시험.
//!
//! 선언(원본 순서): `GitEntry`(APGitEntry) · `ConfigEntry`(APConfigEntry) · `PrepGitWorld`(APPrepGitWorld) · `PrepGit`(APPrepGit, `pin`) ·
//! `parent`(비공개, 뿌리에서 멈춤) · `lexical` · `short_path`(apPrepShortPath) · `fsmonitor_is_bool` · `PREP_GIT_READ_ENV`(apPrepGitReadEnv) ·
//! `prep_git_env`(apPrepGitEnv) · `lfs_filter`(apLFSFilter) · `prep_git_check`(apPrepGitCheck) · `prep_git_block_text`(apPrepGitBlockText).
//!
//! Windows 판단(원본에 없는 것, 모두 시험으로 지킨다):
//! - 판정용 경로: 어댑터가 준 실제 경로(`real`)·`.git` 파일이 적은 경로·include 값은 범위·접두 견주기 전에 모두 `/` 한 가지(`canon`)로 맞춘다.
//!   그러지 않으면 `\` 꼴 경로가 `hasPrefix(… + "/")` 견주기를 빠져나간다(macOS 보다 느슨해지는 구멍). 결과(`top`·`git_dir`·`common_dir`)와 사유 글의 경로도 이 꼴이다(include 값만 적힌 그대로 보이고, 어댑터에도 그대로 넘긴다 — git 이 `..` 를 파일 시스템에서 풀기 때문).
//!   견줄 때의 소문자 접기는 원본이 접는 자리(범위·임시 폴더 목록)에만 있고, 홈 폴더 건너뛰기와 되짚기 견주기는 원본처럼 글자 그대로다(닫힌 쪽).
//! - 뿌리: 위로 올라가는 반복은 `/`·드라이브 뿌리(`C:/`)·UNC 공유 뿌리(`//서버/공유/`)에서 멈춘다. 뿌리 자체의 `.git` 은 본다.
//! - 절대 경로: `/` 로 시작하거나 `\` 로 시작하거나 드라이브(`C:\`·`C:/`)로 시작하면 절대 경로다(`.git` 파일 gitdir·되짚기·commondir·include).
//! - 임시 폴더 목록에 `<홈>/AppData/Local/Temp` 를 더한다(윈도 임시 폴더, 원본에는 mac 자리만 있다).
//! - 지난 실행 기록은 키를 같은 꼴로 맞춰 대소문자 가리지 않고 찾는다(못 찾아서 견주기를 건너뛰는 일을 막는다).
//! - 환경: 윈도는 환경 변수 이름의 대소문자를 가리지 않으므로 `git_config_parameters` 같은 소문자 꼴도 `GIT_*` 로 보고 뺀다(그대로 두면
//!   git 이 읽는다 — macOS 보다 느슨해지는 구멍).
//! - `lexical`: `\`·드라이브(`C:\`)·`\\?\`·`\\.\`·UNC(`\\서버\공유`)·뿌리 없는 `\x` 꼴은 `/` 구분자 한 가지로 맞추고 `\\?\` 접두는 벗긴다.
//!   뒤에 붙은 점·공백(윈도가 무시하는 꼴)은 뗀다. 같은 자리가 같은 글이 되도록 한쪽(막는 쪽)으로만 기운다. 견줄 때의 소문자 접기는 부르는 쪽 몫(원본과 같다).
//! - `short_path`: 홈이 윈도 꼴(드라이브·UNC)이면 접두를 대소문자 가리지 않고, 구분자는 `/`·`\` 둘 다 받는다. 결과는 `~` 뒤에 실제 구분자를 그대로 둔다. 보이는 글일 뿐 판정에 쓰지 않는다.

use super::board_git::{board_git_changed, is_root, parent_path, GitRecord};
use super::gitlinks::{has_submodule, submodule_reason};
use std::collections::{BTreeMap, BTreeSet};

/// `<폴더>/.git` 의 꼴(따라가지 않고 본 것). 원본: `APGitEntry`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitEntry {
    /// `.git` 폴더
    Dir,
    /// `.git` 파일 — "gitdir:" 뒤 글(그 꼴이 아니거나 못 읽으면 None)
    File(Option<String>),
    /// 바로 가기(심볼릭 링크·Windows 정션·재분석 지점)
    Link,
    /// FIFO·장치 등
    Other,
}

/// git 설정 한 줄(`git config --null --list` 꼴: 키는 git 이 소문자로 준 것, 값 없는 키는 None). 원본: `APConfigEntry`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: String,
    pub value: Option<String>,
}

impl ConfigEntry {
    pub fn new(key: &str, value: Option<&str>) -> Self {
        ConfigEntry { key: key.to_string(), value: value.map(str::to_string) }
    }
}

/// 파일 시스템·git 을 보는 포트. 원본: `APPrepGitWorld`
pub trait PrepGitWorld {
    /// `<dir>/.git` 의 꼴(lstat). 없으면 None
    fn entry(&self, dir: &str) -> Option<GitEntry>;
    /// 실제 경로(realpath). 없으면 None
    fn real(&self, path: &str) -> Option<String>;
    /// 작은 보통 파일의 글(바로 가기·FIFO·큰 파일은 None)
    fn text(&self, path: &str) -> Option<String>;
    /// 보통 파일인가 · 폴더인가(lstat, 바로 가기는 아님)
    fn is_file(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    /// "장치:inode"(lstat). Windows 는 "볼륨 일련번호:파일 인덱스"
    fn stamp(&self, path: &str) -> Option<String>;
    /// `git config --file <파일> --no-includes --null --list`(저장소를 찾지 않는 자리에서). 파일이 없으면 `Some(vec![])`, 보통 파일이 아니거나 읽지 못하면 None
    fn config(&self, file: &str) -> Option<Vec<ConfigEntry>>;
    /// 박는 env(pin: GIT_DIR·GIT_COMMON_DIR·GIT_WORK_TREE 와 `PREP_GIT_READ_ENV`)로 색인의 gitlink(`ls-files -s -z`)와
    /// .gitmodules(작업 트리·색인·HEAD) 를 본다. 반환은 `(gitlinks, gitmodules)`(원본의 이름 붙은 튜플). 색인을 읽지 못하면 None
    fn submodules(&self, top: &str, pin: &[String]) -> Option<(Vec<String>, bool)>;
}

/// 경로 하나의 저장소와 막을 까닭. 원본: `APPrepGit`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrepGit {
    /// 저장소 뿌리(파일 시스템으로 찾은 실제 경로). None 이면 git 저장소가 아님(위로 뿌리까지 .git 없음)
    pub top: Option<String>,
    /// 그 자리의 git 폴더(따로 작업 트리면 .git/worktrees/<이름>)
    pub git_dir: Option<String>,
    /// 공통 git 폴더(가지·객체·config)
    pub common_dir: Option<String>,
    /// 막은 까닭 — 있으면 이 저장소에서 git 을 더 돌리지 않는다
    pub block: Option<String>,
}

impl PrepGit {
    /// 통과한 저장소의 읽기(지금 가지·가지 목록·status·rev-list)에 박는 env — git 이 위로 저장소를 스스로 찾지 않게, 원격으로 객체를 받지 않게(`PREP_GIT_READ_ENV`).
    /// 공통 git 폴더도 확인한 자리로 박는다(GIT_COMMON_DIR 가 있으면 git 은 git 폴더의 commondir 파일을 읽지 않는다 — 확인 뒤에 commondir 를 심어도 따르지 않게).
    /// 막혔거나 뿌리·git 폴더를 모르면 빈 목록. 원본: `APPrepGit.pin`
    pub fn pin(&self) -> Vec<String> {
        let (None, Some(t), Some(g)) = (&self.block, &self.top, &self.git_dir) else { return Vec::new() };
        let common = self.common_dir.as_ref().unwrap_or(g);
        let mut env = vec![format!("GIT_DIR={g}"), format!("GIT_COMMON_DIR={common}"), format!("GIT_WORK_TREE={t}")];
        env.extend(PREP_GIT_READ_ENV.iter().map(|s| s.to_string()));
        env
    }
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

fn is_drive(b: &[u8]) -> bool {
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/' || b[2] == b'\\')
}

/// 윈도 꼴 경로인가 — 드라이브(`C:`·`C:\`·`C:/`)나 `\`로 시작(`\\서버`·`\\?\`·`\x`). `/` 로만 쓴 `//a` 는 POSIX 로 본다(원본은 `//a` 를 `/a` 로 접는다).
fn is_windows_style(p: &str) -> bool {
    is_drive(p.as_bytes()) || p.starts_with('\\')
}

/// 경로의 `.`·`..`·겹친 구분자·끝 구분자를 글자만으로 정리한 꼴. 원본: `lexical`(= `URL(fileURLWithPath:).standardizedFileURL.path`).
/// 원본이 `/private` 접두를 실제 파일 시스템을 보고 벗기는 일은 하지 않는다(그 일은 `real` 몫이고 원본 범위 목록이 둘 다 든다).
/// 원본은 상대 경로를 작업 폴더 기준으로 풀지만 부르는 쪽은 늘 절대 경로를 준다 — 여기서는 상대 경로를 그대로 정리만 한다(`..` 는 앞에서 버린다).
/// Windows 꼴은 구분자를 `/` 한 가지로(`C:\a\..\b` → `C:/b`, `\\?\C:\a` → `C:/a`, `\\srv\sh\x` → `//srv/sh/x`, `\\?\UNC\srv\sh` → `//srv/sh/`),
/// 성분 끝의 점·공백을 떼고(윈도는 `a.` 를 `a` 로 본다), 뿌리(`C:/`·`//서버/공유/`·`/`) 위로는 올라가지 않는다.
pub fn lexical(p: &str) -> String {
    let win = is_windows_style(p);
    let (prefix, rooted, rest): (String, bool, &str) = if win { split_windows_prefix(p) } else { (String::new(), p.starts_with('/'), p) };
    let mut stack: Vec<&str> = Vec::new();
    for c in rest.split(|ch: char| if win { is_sep(ch) } else { ch == '/' }) {
        let c = if win && c != "." && c != ".." { c.trim_end_matches(['.', ' ']) } else { c };
        match c {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            _ => stack.push(c),
        }
    }
    let body = stack.join("/");
    if rooted {
        format!("{prefix}/{body}")
    } else {
        format!("{prefix}{body}")
    }
}

/// Windows 꼴 경로를 (접두, 뿌리가 있나, 접두 뒤 나머지)로 가른다. 접두는 `C:`·`//서버/공유`·빈 글이고 구분자는 `/` 로 맞춘다.
fn split_windows_prefix(p: &str) -> (String, bool, &str) {
    // `\\?\`·`\\.\` — 뒤가 `UNC\서버\공유\…` 이면 UNC, 아니면 드라이브(또는 그 밖의 장치)로 읽는다
    for ns in [r"\\?\", r"\\.\", "//?/", "//./"] {
        if let Some(rest) = p.strip_prefix(ns) {
            if rest.get(..3).is_some_and(|h| h.eq_ignore_ascii_case("unc")) && rest[3..].starts_with(is_sep) {
                return split_unc(&rest[4..]);
            }
            // 접두 뒤는 늘 절대 자리다(`\\?\한글\x` 처럼 드라이브가 없어도)
            let (pre, rooted, r) = split_drive_or_root(rest);
            let rooted = rooted || pre.is_empty();
            return (pre, rooted, r);
        }
    }
    if let Some(after) = p.strip_prefix(r"\\") {
        return split_unc(after);
    }
    split_drive_or_root(p)
}

/// `서버\공유\나머지` → (`//서버/공유`, true, 나머지). 공유가 없으면 (`//서버`, true, "")
fn split_unc(after: &str) -> (String, bool, &str) {
    let mut it = after.splitn(3, is_sep);
    let server = it.next().unwrap_or("");
    let share = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    let prefix = if share.is_empty() { format!("//{server}") } else { format!("//{server}/{share}") };
    (prefix, true, rest)
}

fn split_drive_or_root(p: &str) -> (String, bool, &str) {
    if is_drive(p.as_bytes()) {
        let rooted = p.len() > 2;
        return (p[..2].to_string(), rooted, &p[2..]);
    }
    (String::new(), p.starts_with(is_sep), p)
}

/// 보이는 글의 홈 줄임: `<홈>/…` 이면 `~/…`. 홈이 윈도 꼴이면 접두는 대소문자를 가리지 않고 구분자는 `/`·`\` 둘 다 받으며 `~` 뒤에 실제 구분자를 둔다.
/// 원본: `apPrepShortPath`
pub fn short_path(p: &str, home: &str) -> String {
    if is_windows_style(home) {
        if let Some(head) = p.get(..home.len()) {
            let same = head.len() == home.len()
                && head.bytes().zip(home.bytes()).all(|(a, b)| a.eq_ignore_ascii_case(&b) || (is_sep(a as char) && is_sep(b as char)));
            if same {
                let rest = &p[home.len()..];
                if rest.starts_with(is_sep) {
                    return format!("~{rest}");
                }
            }
        }
        return p.to_string();
    }
    match p.strip_prefix(home).and_then(|r| r.strip_prefix('/')) {
        Some(rest) => format!("~/{rest}"),
        None => p.to_string(),
    }
}

/// 명령이 아닌 `core.fsmonitor` 값(끔·켬, git 의 불리언 꼴). 켬(true)은 git 이 든 내장 감시이고, 그 밖의 글은 돌릴 명령(hook 경로)이다.
/// 값 없음(None)은 불리언. 앞뒤를 자르는 글자는 원본 `.whitespaces`(Unicode Zs 와 탭)와 같다 — Rust 의 `trim()` 은 줄바꿈도 잘라
/// `"false\n"` 을 불리언으로 받아 원본보다 느슨해진다. 원본: `fsmonitorIsBool`
pub fn fsmonitor_is_bool(v: Option<&str>) -> bool {
    let Some(v) = v else { return true };
    let t = v.trim_matches(is_swift_whitespace).to_lowercase();
    ["", "true", "false", "yes", "no", "on", "off", "1", "0"].contains(&t.as_str())
}

/// Swift `CharacterSet.whitespaces` — 일반 범주 Zs 와 탭
fn is_swift_whitespace(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

/// 출발 준비의 git 읽기는 부분 복제(promisor) 저장소에서 빠진 객체를 받으려고 원격 명령(remote.*.uploadpack·core.sshCommand)을 부르지 않게
/// 늦은 받기를 끄고 모든 전송 프로토콜을 막는다(GIT_CONFIG_COUNT 로 명령줄 설정과 같은 무게). 원본: `apPrepGitReadEnv`
pub const PREP_GIT_READ_ENV: [&str; 4] = ["GIT_NO_LAZY_FETCH=1", "GIT_CONFIG_COUNT=1", "GIT_CONFIG_KEY_0=protocol.allow", "GIT_CONFIG_VALUE_0=never"];

/// 출발 준비 읽기의 환경 — 보드가 물려받은 `GIT_*`(GIT_CONFIG_PARAMETERS·GIT_CONFIG_COUNT/KEY/VALUE·GIT_SSH*·GIT_ALLOW_PROTOCOL·GIT_DIR·GIT_INDEX_FILE 등)는
/// 모두 빼고, 시스템·전역 설정·attributes·replace 를 읽지 않는 보호(보드 `apGitEnv` 와 같은 넷)와 박는 값(`extra`: "이름=값")만 넣는다.
/// 이름이 `GIT_` 로 시작하는지는 대소문자를 가리지 않는다(윈도 환경 변수 이름은 대소문자를 가리지 않는다 — 원본은 대문자만 뺀다).
/// `extra` 의 칸은 첫 `=` 에서 가르고 `=` 가 없으면 버린다. 원본: `apPrepGitEnv`
pub fn prep_git_env(inherited: &BTreeMap<String, String>, extra: &[String]) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> =
        inherited.iter().filter(|(k, _)| !k.to_ascii_uppercase().starts_with("GIT_")).map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in [("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")] {
        env.insert(k.to_string(), v.to_string());
    }
    for s in extra {
        if let Some((k, v)) = s.split_once('=') {
            env.insert(k.to_string(), v.to_string());
        }
    }
    env
}

/// git-lfs 가 저장소에 쓰는 표준 filter 글(`git lfs install --local`) — 이 글 그대로일 때만 filter 명령을 받는다. 키는 소문자. 원본: `apLFSFilter`(사전)
/// 원본은 `apLFSFilter[k] == e.value` 로 견주므로 표에 없는 키와 값 없는 설정(None)도 같다고 나온다 — 부르는 쪽은 `lfs_filter(k) == value` 로 그대로 견준다.
pub fn lfs_filter(key: &str) -> Option<&'static str> {
    match key {
        "filter.lfs.clean" => Some("git-lfs clean -- %f"),
        "filter.lfs.smudge" => Some("git-lfs smudge -- %f"),
        "filter.lfs.process" => Some("git-lfs filter-process"),
        _ => None,
    }
}

/// `/` 아래 경로 뒤에 `/` 를 붙인 접두(이미 `/` 로 끝나면 그대로 — 뿌리 `/`·`C:/` 에서 `//` 가 되지 않게). 원본은 `p + "/"` 로 뿌리에서 `//` 가 된다.
fn with_slash(p: &str) -> String {
    if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    }
}

fn join(dir: &str, name: &str) -> String {
    format!("{}{name}", with_slash(dir))
}

/// 절대 경로인가 — `/`·`\` 로 시작하거나 드라이브(`C:\`·`C:/`)로 시작. 드라이브만(`C:`)이면 상대다
fn is_abs(p: &str) -> bool {
    p.starts_with(is_sep) || (is_drive(p.as_bytes()) && p.len() > 2)
}

/// 판정에 쓰기 전 경로를 `/` 한 가지로 맞춘다. `lexical` 과 같되 이미 맞춘 UNC 꼴(`//서버/공유/…`)과 `//?/`·`//./` 접두를 UNC·장치 접두로 읽는다.
/// 맞춘 글에 다시 써도 같다(`lexical` 은 `//a` 를 `/a` 로 접어 UNC 를 깨뜨리므로 맞춘 글에는 쓰지 않는다)
fn canon(p: &str) -> String {
    if let Some(r) = p.strip_prefix("//") {
        if !r.is_empty() && !r.starts_with('/') {
            let win = if r.starts_with("?/") || r.starts_with("./") { format!(r"\\{}\{}", &r[..1], &r[2..]) } else { format!(r"\\{r}") };
            return lexical(&win);
        }
    }
    lexical(p)
}

fn real_c(world: &dyn PrepGitWorld, p: &str) -> Option<String> {
    world.real(p).map(|r| canon(&r))
}

fn abs_or_join(t: &str, base: &str) -> String {
    if is_abs(t) {
        t.to_string()
    } else {
        join(base, t)
    }
}

/// `//서버/공유` 꼴(공유 뿌리)인가
fn is_unc_share(p: &str) -> bool {
    let Some(r) = p.strip_prefix("//") else { return false };
    let mut it = r.trim_end_matches('/').split('/');
    matches!((it.next(), it.next(), it.next()), (Some(s), Some(sh), None) if !s.is_empty() && !sh.is_empty())
}

/// 더 올라갈 데가 없는 뿌리인가 — `/`·드라이브 뿌리·UNC 공유 뿌리
fn at_root(p: &str) -> bool {
    p == "/" || is_root(p) || is_unc_share(p)
}

/// 원본 `parent`: 부모 경로. 비면 `/`. 드라이브·UNC 공유의 부모는 뿌리 꼴(`C:/`·`//서버/공유/`)로 돌려준다(`C:` 는 드라이브의 현재 폴더라 뿌리가 아니다)
fn parent(p: &str) -> String {
    let d = parent_path(p);
    let b = d.as_bytes();
    if d.is_empty() {
        "/".to_string()
    } else if (b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':') || is_unc_share(&d) {
        with_slash(&d)
    } else {
        d
    }
}

/// 맞춘 경로를 (뿌리 접두, 성분들)로 가른다. 접두는 `""`(POSIX)·`C:`·`//서버/공유`
fn split_root(p: &str) -> (String, Vec<String>) {
    let parts = |s: &str| s.split('/').filter(|c| !c.is_empty()).map(str::to_string).collect::<Vec<_>>();
    if let Some(r) = p.strip_prefix("//") {
        let mut v = parts(r);
        let take = v.len().min(2);
        let head: Vec<String> = v.drain(..take).collect();
        return (format!("//{}", head.join("/")), v);
    }
    if is_drive(p.as_bytes()) {
        return (p[..2].to_string(), parts(&p[2..]));
    }
    (String::new(), parts(p))
}

fn blocked(r: &PrepGit, why: String) -> PrepGit {
    PrepGit { block: Some(why), ..r.clone() }
}

fn clip(v: &str, n: usize) -> String {
    if v.chars().count() > n {
        format!("{}…", v.chars().take(n).collect::<String>())
    } else {
        v.to_string()
    }
}

/// git 설정 파일을 따라가며 보는 자리에 필요한 값
struct ConfigScan<'a> {
    world: &'a dyn PrepGitWorld,
    home: &'a str,
    home_c: &'a str,
    scope: &'a [String],
}

impl ConfigScan<'_> {
    /// 이 경로가 저장소·임시 폴더 안인가 — 글자 정리(`/` 접기·UNC 읽기 둘 다)와 실제 경로를 모두 소문자로 접어 견준다
    fn in_scope(&self, p: &str) -> bool {
        let forms = [lexical(p), canon(p), real_c(self.world, p).unwrap_or_else(|| canon(p))];
        forms.iter().map(|f| f.to_lowercase()).any(|f| self.scope.iter().any(|s| f == *s || f.starts_with(&with_slash(s))))
    }

    fn walk(&self, file: &str, depth: usize, root: bool, seen: &mut BTreeSet<String>) -> Option<String> {
        if !seen.insert(real_c(self.world, file).unwrap_or_else(|| canon(file))) {
            return None;
        }
        let home = self.home;
        let Some(entries) = self.world.config(file) else { return Some(format!("git 설정({})을 읽지 못해", short_path(file, home))) };
        let dir = parent(&real_c(self.world, file).unwrap_or_else(|| canon(file)));
        for e in entries {
            let k = e.key.to_lowercase();
            if root && k == "core.fsmonitor" && !fsmonitor_is_bool(e.value.as_deref()) {
                let v = e.value.clone().unwrap_or_default();
                return Some(format!("저장소 설정({})에 core.fsmonitor 명령({})이 있어", short_path(file, home), clip(&v, 60)));
            }
            // filter 드라이버의 명령(clean·smudge·process)은 git-lfs 표준 글만 받는다. include 로 끌어온 파일도 같다.
            // 원본은 `apLFSFilter[k] == e.value` — 값 없는 설정(None)은 표에 없는 키와 같게 나와 막히지 않는다(그대로 옮김)
            if k.starts_with("filter.") && (k.ends_with(".clean") || k.ends_with(".smudge") || k.ends_with(".process")) && lfs_filter(&k) != e.value.as_deref() {
                return Some(format!("{}({})에 filter 명령({k})이 있어", if root { "저장소 설정" } else { "git 설정" }, short_path(file, home)));
            }
            if !(k == "include.path" || (k.starts_with("includeif.") && k.ends_with(".path"))) {
                continue;
            }
            let Some(v) = e.value.as_deref().filter(|v| !v.is_empty()) else { continue };
            // git 은 ~<사용자>/ 를 그 사용자의 홈으로, %(prefix)/ 를 git 설치 자리로 푼다 — 우리 풀이와 달라 닫힌 쪽으로 막는다
            if (v.starts_with('~') && v != "~" && !v.starts_with("~/")) || v.contains("%(prefix)") {
                return Some(format!("git 설정 include 경로({})를 git 과 같게 풀 수 없어", clip(v, 80)));
            }
            let p = if v == "~" {
                self.home_c.to_string()
            } else if let Some(rest) = v.strip_prefix("~/") {
                join(self.home_c, rest)
            } else if is_abs(v) {
                v.to_string()
            } else {
                join(&dir, v)
            };
            if self.in_scope(&p) {
                return Some(format!("git 설정 include 가 저장소·임시 폴더 안({})을 가리켜", short_path(&p, home)));
            }
            if depth >= 10 {
                return Some("git 설정 include 가 10단 넘게 겹쳐".to_string());
            }
            if let Some(why) = self.walk(&p, depth + 1, false, seen) {
                return Some(why);
            }
        }
        None
    }
}

/// 유스케이스: 경로 하나(고른 작업 경로·단위 작업 폴더)의 저장소와 막을 까닭. `records` 는 이어 가는 지난 실행의 저장소별 git 자리 기록(뿌리 → 기록).
/// 반환의 경로(`top`·`git_dir`·`common_dir`)와 사유 글은 `/` 한 가지로 맞춘 꼴이다(모듈 머리 주석). 원본: `apPrepGitCheck`
pub fn prep_git_check(path: &str, world: &dyn PrepGitWorld, records: &BTreeMap<String, GitRecord>, home: &str) -> PrepGit {
    let Some(start) = real_c(world, path) else { return PrepGit::default() };
    let home_c = canon(home);
    let home_slash = with_slash(&home_c);
    let skip_home = |up: &str| up == home_c || home_slash.starts_with(&with_slash(up));
    // ① 위로 올라가며 처음 만난 .git — git 이 저장소로 보지 않는 .git 폴더(HEAD 없음)는 git 처럼 지나친다.
    // HEAD 가 있는데 보통 파일이 아니면(바로 가기·FIFO 등) 그 .git 을 찾은 것으로 두고 막는다(닫힌 쪽)
    let mut d = start;
    let mut found: Option<(String, GitEntry)> = None;
    let mut odd_head = false;
    loop {
        if let Some(e) = world.entry(&d) {
            if matches!(e, GitEntry::Dir) && !world.is_file(&join(&d, ".git/HEAD")) {
                if world.stamp(&join(&d, ".git/HEAD")).is_some() {
                    found = Some((d, e));
                    odd_head = true;
                    break;
                }
            } else {
                found = Some((d, e));
                break;
            }
        }
        if at_root(&d) {
            break;
        }
        d = parent(&d);
    }
    let Some((top, entry)) = found else { return PrepGit::default() };
    let mut r = PrepGit { top: Some(top.clone()), ..PrepGit::default() };
    if odd_head {
        return blocked(&r, ".git/HEAD 가 보통 파일이 아니라(바로 가기 등)".to_string());
    }
    let g: String = match &entry {
        GitEntry::Link => return blocked(&r, ".git 이 바로 가기라".to_string()),
        GitEntry::Other => return blocked(&r, ".git 이 폴더도 파일도 아니라".to_string()),
        GitEntry::File(None) => return blocked(&r, ".git 파일을 읽지 못해".to_string()),
        GitEntry::File(Some(raw)) => {
            let t = raw.trim();
            match real_c(world, &abs_or_join(t, &top)).filter(|gg| !t.is_empty() && world.is_dir(gg)) {
                Some(gg) => gg,
                None => return blocked(&r, format!(".git 파일이 가리키는 git 폴더({t})가 없어")),
            }
        }
        GitEntry::Dir => join(&top, ".git"),
    };
    r.git_dir = Some(g.clone());
    // git 폴더 꼴: <뿌리>/.git(또는 bare 꼴 <이름>.git) 그 자체, 그 아래 worktrees/<이름>(따로 작업 트리), modules/…(하위 모듈)
    let (prefix, comps) = split_root(&g);
    let Some(i) = comps.iter().rposition(|c| c == ".git").or_else(|| comps.iter().rposition(|c| c.ends_with(".git"))) else {
        return blocked(&r, format!("git 폴더({})가 저장소 .git 꼴 자리가 아니라", short_path(&g, home)));
    };
    let owner = format!("{prefix}/{}", comps[..=i].join("/"));
    let rest = &comps[i + 1..];
    let base = if comps[i] == ".git" { parent(&owner) } else { owner.clone() }; // 그 git 폴더를 품은 저장소 뿌리(bare 꼴이면 그 폴더)
    // 보안 검토 고침 1: 고른 폴더(뿌리) 위로도 본다 — 다른 저장소의 작업 트리 안이면 막는다. 그 저장소 자신의 따로 작업 트리·하위 모듈만 예외. 홈 폴더와 그 위는 보지 않는다
    let mut up = parent(&top);
    while !at_root(&up) && !skip_home(&up) {
        if world.entry(&up).is_some() && !(join(&up, ".git") == owner && !rest.is_empty()) {
            return blocked(&r, format!("다른 저장소({})의 작업 트리 안에 든 git 자리라", short_path(&up, home)));
        }
        up = parent(&up);
    }
    // 보안 검토 고침 2: .git 파일이 가리키는 git 폴더는 모델이 쓸 수 있던 자리면 막는다 — 그 작업 트리 안, 임시 폴더, 자율주행 작업 트리 자리.
    // 떨어진 git 폴더(--separate-git-dir 꼴)는 되짚는 자리가 없어 누가 만들었는지 가릴 수 없으므로 막는다(닫힌 쪽)
    let writable: Vec<String> = [
        "/tmp",
        "/private/tmp",
        "/var/tmp",
        "/private/var/tmp",
        "/var/folders",
        "/private/var/folders",
        &join(&home_c, ".claude-board/worktrees"),
        &join(&home_c, "AppData/Local/Temp"),
    ]
    .iter()
    .map(|s| s.to_lowercase())
    .collect();
    if matches!(entry, GitEntry::File(_)) {
        if g.starts_with(&with_slash(&top)) {
            return blocked(&r, format!("git 폴더({})가 그 작업 트리 안에 있어", short_path(&g, home)));
        }
        for x in [&g, &owner] {
            let xl = x.to_lowercase();
            if writable.iter().any(|w| xl == *w || xl.starts_with(&with_slash(w))) {
                return blocked(&r, format!("git 폴더({})가 임시 폴더·자율주행 작업 트리 자리에 있어", short_path(&g, home)));
            }
        }
        if rest.is_empty() {
            return blocked(&r, format!("git 파일이 떨어진 git 폴더({})를 가리켜 자리를 믿을 수 없어", short_path(&g, home)));
        }
    }
    // 보안 검토 고침 8: git 폴더에 commondir 파일이 있으면 git 은 그 자리를 공통 git 폴더로 써서 거기 config 를 읽는다.
    // commondir 는 따로 작업 트리(worktrees/<이름>)에만 받고, 그 밖의 git 폴더(.git 그대로·modules/…)에 있으면 막는다. 있는지는 lstat(바로 가기·FIFO 도 있음으로)
    let has_commondir = world.stamp(&join(&g, "commondir")).is_some();
    let mut common = g.clone();
    if rest.is_empty() {
        if has_commondir {
            return blocked(&r, format!("git 폴더({})에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려", short_path(&g, home)));
        }
    } else if rest.len() == 2 && rest[0] == "worktrees" {
        // 따로 작업 트리: 되짚는 자리(gitdir)가 이 .git 파일이고 commondir 가 그 git 폴더여야 한다
        let back = world.text(&join(&g, "gitdir")).map(|s| s.trim().to_string()).unwrap_or_default();
        let back_real = if back.is_empty() { None } else { real_c(world, &abs_or_join(&back, &g)) };
        if back_real.as_deref() != Some(join(&top, ".git").as_str()) {
            return blocked(&r, "따로 작업 트리의 되짚는 자리(gitdir)가 이 폴더가 아니라".to_string());
        }
        let cd = world.text(&join(&g, "commondir")).map(|s| s.trim().to_string()).unwrap_or_default();
        if cd.is_empty() || real_c(world, &abs_or_join(&cd, &g)).as_deref() != Some(owner.as_str()) {
            return blocked(&r, "따로 작업 트리의 commondir 가 그 저장소 git 폴더가 아니라".to_string());
        }
        common = owner.clone();
    } else if rest.len() >= 2 && rest[0] == "modules" {
        if !top.starts_with(&with_slash(&base)) {
            return blocked(&r, "하위 모듈 git 폴더가 그 저장소 밖을 가리켜".to_string());
        }
        if has_commondir {
            return blocked(&r, format!("하위 모듈 git 폴더({})에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려", short_path(&g, home)));
        }
    } else {
        return blocked(&r, format!("git 폴더({})가 저장소 .git 꼴 자리가 아니라", short_path(&g, home)));
    }
    r.common_dir = Some(common.clone());
    // ② 다른 저장소의 작업 트리 안에 든 git 폴더(중첩 git)인가 — 홈 폴더와 그 위는 보지 않는다
    let mut a = parent(&base);
    while !at_root(&a) && !skip_home(&a) {
        if world.entry(&a).is_some() {
            return blocked(&r, format!("다른 저장소({})의 작업 트리 안에 든 git 자리라", short_path(&a, home)));
        }
        a = parent(&a);
    }
    // ③ 이어 가는 지난 실행의 기록과 같은 git 폴더인가(공통 git 폴더만). 기록의 키는 같은 꼴로 맞춰 대소문자 가리지 않고 찾는다
    let top_l = top.to_lowercase();
    for (key, rec) in records {
        if canon(key).to_lowercase() != top_l {
            continue;
        }
        let rec = GitRecord { common_dir: rec.common_dir.clone(), common_ino: rec.common_ino.clone(), work_git_dir: None, work_git_ino: None };
        let live_ino = world.stamp(&common);
        if let Some(why) = board_git_changed(&rec, Some(&common), live_ino.as_deref(), None, false) {
            return blocked(&r, why.replace("출발 때", "지난 실행이 출발할 때"));
        }
    }
    // ④ git 설정: 명령인 core.fsmonitor, 저장소·임시 폴더 안을 가리키는 include
    let scope_src: Vec<String> = vec![
        top.clone(),
        base.clone(),
        common.clone(),
        g.clone(),
        join(&home_c, ".claude-board/worktrees"),
        "/tmp".to_string(),
        "/private/tmp".to_string(),
        "/var/tmp".to_string(),
        "/private/var/tmp".to_string(),
        "/var/folders".to_string(),
        "/private/var/folders".to_string(),
        join(&home_c, "AppData/Local/Temp"),
    ];
    let scope: Vec<String> = scope_src
        .iter()
        .flat_map(|x| [canon(x), real_c(world, x).unwrap_or_else(|| canon(x))])
        .map(|s| s.to_lowercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let scan = ConfigScan { world, home, home_c: &home_c, scope: &scope };
    let mut seen = BTreeSet::new();
    let mut files = vec![join(&common, "config"), join(&common, "config.worktree")];
    if g != common {
        files.push(join(&g, "config.worktree"));
    }
    for f in files {
        if let Some(why) = scan.walk(&f, 0, true, &mut seen) {
            return blocked(&r, why);
        }
    }
    // ⑤ 하위 모듈(색인 gitlink·.gitmodules) — 엔진이 출발하지 않는 저장소. 색인을 못 읽으면 닫힌 쪽
    let Some((gitlinks, gitmodules)) = world.submodules(&top, &r.pin()) else {
        return blocked(&r, "색인을 읽지 못해(git ls-files 실패)".to_string());
    };
    if has_submodule(&gitlinks, gitmodules) {
        return blocked(&r, submodule_reason(&gitlinks, gitmodules));
    }
    r
}

/// 저장소 줄·준비 줄에 보이는 글(엔진의 "보안: <저장소> 의 <까닭> git 을 돌리지 않습니다" 와 같은 꼴). 원본: `apPrepGitBlockText`
pub fn prep_git_block_text(top: &str, why: &str, home: &str) -> String {
    format!("보안: {} 의 {why} 그 저장소에서 git 을 돌리지 않았습니다. 사람이 확인한 뒤 다시 읽습니다.", short_path(top, home))
}
