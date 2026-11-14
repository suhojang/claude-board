//! 출발 준비 git 자리 확인(`rules::prep_git`) 시험.
//! 원본: `Tests/AutopilotRulesTests/PrepGitTests.swift` — Swift 시험 다섯(`확인` · `공통폴더박기` · `읽기env` · `물려받은env` · `글`)을 같은 이름으로 옮겼다.
//! `확인` 의 표(`rows`, 60행)와 가짜 `FakeGitFS` 는 Swift 원본과 같은 글이다(표는 변환 스크립트로 옮겼다).
//! 끝의 「추가」 절은 Swift 시험에 없는 이 이식의 몫(Windows 경로·뿌리·범위 견주기·환경 이름·원본 동작 고정)이다.

use claude_board_autopilot::rules::board_git::GitRecord;
use claude_board_autopilot::rules::prep_git::{
    fsmonitor_is_bool, lexical, lfs_filter, prep_git_block_text, prep_git_check, prep_git_env, short_path, ConfigEntry, GitEntry, PrepGit,
    PrepGitWorld, PREP_GIT_READ_ENV,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 가짜 파일 시스템(폴더·파일·바로 가기·FIFO)과 가짜 git(설정 줄·색인). 원본 `FakeGitFS` 와 같다.
/// 더한 것: 경로를 `lexical` 로 맞춰 푼다(`C:\w\a`·`\\srv\sh\x` 꼴도 받는다), `entry` 로 물어 본 폴더 기록(`entry_calls`), 대소문자 가리지 않기(`ci`)
#[derive(Default)]
struct FakeGitFS {
    dirs: BTreeSet<String>,
    files: BTreeMap<String, String>,
    links: BTreeMap<String, String>,
    fifos: BTreeSet<String>,
    configs: BTreeMap<String, Vec<ConfigEntry>>,
    index: BTreeMap<String, (Vec<String>, bool)>,
    index_fail: BTreeSet<String>,
    stamps: BTreeMap<String, String>,
    /// 색인 읽기(submodules)에 받은 박는 env
    pins: RefCell<BTreeMap<String, Vec<String>>>,
    entry_calls: RefCell<Vec<String>>,
    ci: bool,
    /// `real` 이 `\` 구분자로 돌려준다(윈도 어댑터의 실제 경로 꼴)
    bs: bool,
}

fn norm(p: &str) -> String {
    // 맞춘 UNC(`//서버/공유/…`)는 `\\` 로 바꿔 `lexical` 에 준다(`lexical` 은 `//a` 를 `/a` 로 접는다)
    match p.strip_prefix("//") {
        Some(r) if !r.is_empty() && !r.starts_with('/') => lexical(&format!(r"\\{r}")),
        _ => lexical(p),
    }
}

fn is_drive_comp(c: &str) -> bool {
    c.len() == 2 && c.as_bytes()[0].is_ascii_alphabetic() && c.as_bytes()[1] == b':'
}

/// 맞춘 경로의 (성분, 누적 경로들). POSIX `/w/a` → (w, a), (/w, /w/a). 드라이브 `C:/w` → (C:, w), (C:, C:/w). UNC `//s/h/x` → (s, h, x), (//s, //s/h, //s/h/x)
fn walk(n: &str) -> (Vec<String>, Vec<String>) {
    let comps: Vec<String> = n.split('/').filter(|c| !c.is_empty()).map(str::to_string).collect();
    let mut cur = if n.starts_with("//") { "/".to_string() } else { String::new() };
    let mut cums = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        cur = if i == 0 && is_drive_comp(c) { c.clone() } else { format!("{cur}/{c}") };
        cums.push(cur.clone());
    }
    (comps, cums)
}

fn jn(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

impl FakeGitFS {
    fn new() -> Self {
        let mut f = FakeGitFS::default();
        f.dirs.insert("/".to_string());
        f
    }
    fn mkdir(&mut self, p: &str) {
        let (_, cums) = walk(&norm(p));
        for c in cums {
            self.dirs.insert(c);
        }
    }
    fn put(&mut self, p: &str, text: &str) {
        self.mkdir(p.rsplit_once('/').map_or("", |(d, _)| d));
        self.files.insert(p.to_string(), text.to_string());
    }
    fn repo(&mut self, r: &str) {
        self.mkdir(&jn(r, ".git"));
        self.put(&jn(r, ".git/HEAD"), "ref: refs/heads/main\n");
        self.put(&jn(r, ".git/config"), "");
    }
    fn cfg(&mut self, file: &str, kv: &[(&str, Option<&str>)]) {
        if !self.files.contains_key(file) {
            self.put(file, "");
        }
        self.configs.insert(file.to_string(), kv.iter().map(|(k, v)| ConfigEntry::new(k, *v)).collect());
    }
    fn resolve(&self, p: &str, hops: usize) -> Option<String> {
        let (comps, cums) = walk(&norm(p));
        for (i, cur) in cums.iter().enumerate() {
            if let Some(t) = self.links.get(cur) {
                if hops >= 16 {
                    return None;
                }
                let rest = comps[i + 1..].join("/");
                return self.resolve(&format!("{t}{}", if rest.is_empty() { String::new() } else { format!("/{rest}") }), hops + 1);
            }
        }
        let cur = cums.last().cloned().unwrap_or_else(|| "/".to_string());
        let exists = |k: &String| self.dirs.contains(k) || self.files.contains_key(k) || self.fifos.contains(k);
        if exists(&cur) {
            // 드라이브 뿌리의 실제 경로는 `C:/`(윈도 어댑터가 `C:\` 로 준다)
            return Some(if is_drive_comp(&cur) { format!("{cur}/") } else { cur });
        }
        if self.ci {
            let all = self.dirs.iter().chain(self.files.keys()).chain(self.fifos.iter());
            return all.into_iter().find(|k| k.eq_ignore_ascii_case(&cur)).cloned();
        }
        None
    }
}

impl PrepGitWorld for FakeGitFS {
    fn entry(&self, dir: &str) -> Option<GitEntry> {
        self.entry_calls.borrow_mut().push(dir.to_string());
        let p = jn(dir, ".git");
        if self.links.contains_key(&p) {
            return Some(GitEntry::Link);
        }
        if self.fifos.contains(&p) {
            return Some(GitEntry::Other);
        }
        if self.dirs.contains(&p) {
            return Some(GitEntry::Dir);
        }
        self.files.get(&p).map(|f| GitEntry::File(f.strip_prefix("gitdir:").map(str::to_string)))
    }
    fn real(&self, path: &str) -> Option<String> {
        self.resolve(path, 0).map(|s| if self.bs { s.replace('/', "\\") } else { s })
    }
    fn text(&self, path: &str) -> Option<String> {
        if self.links.contains_key(path) {
            None
        } else {
            self.files.get(path).cloned()
        }
    }
    fn is_file(&self, path: &str) -> bool {
        !self.links.contains_key(path) && self.files.contains_key(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        !self.links.contains_key(path) && self.dirs.contains(path)
    }
    fn stamp(&self, path: &str) -> Option<String> {
        self.stamps.get(path).cloned().or_else(|| self.resolve(path, 0).map(|_| format!("1:{path}")))
    }
    fn config(&self, file: &str) -> Option<Vec<ConfigEntry>> {
        let norm_file = norm(file);
        let file = norm_file.as_str();
        if self.links.contains_key(file) || self.fifos.contains(file) {
            return None;
        }
        if !self.files.contains_key(file) {
            return Some(Vec::new());
        }
        Some(self.configs.get(file).cloned().unwrap_or_default())
    }
    fn submodules(&self, top: &str, pin: &[String]) -> Option<(Vec<String>, bool)> {
        self.pins.borrow_mut().insert(top.to_string(), pin.to_vec());
        if self.index_fail.contains(top) {
            None
        } else {
            Some(self.index.get(top).cloned().unwrap_or((Vec::new(), false)))
        }
    }
}

struct PrepGitRow {
    note: &'static str,
    path: &'static str,
    home: &'static str,
    records: Vec<(&'static str, GitRecord)>,
    build: fn(&mut FakeGitFS),
    top: Option<&'static str>,
    git_dir: Option<&'static str>,
    /// 막는 까닭(전체가 같아야 한다 — Swift 는 `#expect(got.block == r.block)`)
    block: Option<&'static str>,
}

// 이 표는 `Tests/AutopilotRulesTests/PrepGitTests.swift` 의 `rows` 를 변환 스크립트로 옮긴 것이다(손으로 다시 치지 않음). 행 수: 60
fn rows() -> Vec<PrepGitRow> {
    vec![
        PrepGitRow { note: "보통 저장소 뿌리", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); }, top: Some("/w/a"), git_dir: Some("/w/a/.git"), block: None },
        PrepGitRow { note: "하위 폴더에서 고르면 위로 찾은 뿌리", path: "/w/a/src/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.mkdir("/w/a/src/x"); }, top: Some("/w/a"), git_dir: Some("/w/a/.git"), block: None },
        PrepGitRow { note: "git 저장소가 아님(위로 / 까지 .git 없음)", path: "/w/plain", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.mkdir("/w/plain"); }, top: None, git_dir: None, block: None },
        PrepGitRow { note: "없는 경로", path: "/w/none", home: "/h", records: vec![], build: |_f: &mut FakeGitFS| {}, top: None, git_dir: None, block: None },
        PrepGitRow { note: "HEAD 없는 .git 폴더는 git 처럼 지나친다", path: "/w/a/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.mkdir("/w/a/x/.git"); }, top: Some("/w/a"), git_dir: Some("/w/a/.git"), block: None },
        // 보안 검토 고침(다듬기 ⑬-2, MEDIUM 찾기가 git 과 다름): git 은 HEAD 가 refs/ 를 가리키는 바로 가기여도 저장소로 받는다 — 지나치지 않고 막는다
        PrepGitRow { note: "HEAD 가 바로 가기인 .git(혼자 선 저장소)은 막음", path: "/w/hs", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/hs"); f.files.remove("/w/hs/.git/HEAD"); f.put("/w/hs/.git/refs/heads/main", "0"); f.links.insert("/w/hs/.git/HEAD".to_string(), "/w/hs/.git/refs/heads/main".to_string()); }, top: Some("/w/hs"), git_dir: None, block: Some(".git/HEAD 가 보통 파일이 아니라(바로 가기 등)") },
        PrepGitRow { note: "바깥 저장소 안 중첩 저장소의 HEAD 가 바로 가기면 바깥으로 지나치지 않고 막음", path: "/w/a/app/src", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.repo("/w/a/app"); f.mkdir("/w/a/app/src"); f.files.remove("/w/a/app/.git/HEAD"); f.put("/w/a/app/.git/refs/heads/main", "0"); f.links.insert("/w/a/app/.git/HEAD".to_string(), "/w/a/app/.git/refs/heads/main".to_string()); }, top: Some("/w/a/app"), git_dir: None, block: Some(".git/HEAD 가 보통 파일이 아니라(바로 가기 등)") },
        PrepGitRow { note: "HEAD 가 FIFO 인 .git 도 막음", path: "/w/hf", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/hf"); f.files.remove("/w/hf/.git/HEAD"); f.fifos.insert("/w/hf/.git/HEAD".to_string()); }, top: Some("/w/hf"), git_dir: None, block: Some(".git/HEAD 가 보통 파일이 아니라(바로 가기 등)") },
        PrepGitRow { note: ".git 이 바로 가기면 막음", path: "/w/l", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.mkdir("/w/l"); f.repo("/w/real"); f.links.insert("/w/l/.git".to_string(), "/w/real/.git".to_string()); }, top: Some("/w/l"), git_dir: None, block: Some(".git 이 바로 가기라") },
        PrepGitRow { note: ".git 이 FIFO 면 막음", path: "/w/f", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.mkdir("/w/f"); f.fifos.insert("/w/f/.git".to_string()); }, top: Some("/w/f"), git_dir: None, block: Some(".git 이 폴더도 파일도 아니라") },
        PrepGitRow { note: ".git 파일이 gitdir 꼴이 아니면 막음", path: "/w/g", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.put("/w/g/.git", "hello"); }, top: Some("/w/g"), git_dir: None, block: Some(".git 파일을 읽지 못해") },
        PrepGitRow { note: ".git 파일이 없는 자리를 가리키면 막음", path: "/w/g", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.put("/w/g/.git", "gitdir: /w/nowhere/.git\n"); }, top: Some("/w/g"), git_dir: None, block: Some(".git 파일이 가리키는 git 폴더(/w/nowhere/.git)가 없어") },
        PrepGitRow { note: "따로 작업 트리(저장소 밖) 통과", path: "/w/wt/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/wt/x/.git", "gitdir: /w/a/.git/worktrees/x\n"); f.put("/w/a/.git/worktrees/x/gitdir", "/w/wt/x/.git\n"); f.put("/w/a/.git/worktrees/x/commondir", "../..\n"); f.put("/w/a/.git/worktrees/x/HEAD", ""); }, top: Some("/w/wt/x"), git_dir: Some("/w/a/.git/worktrees/x"), block: None },
        PrepGitRow { note: "작업 트리 안 따로 작업 트리(.claude/worktrees 꼴)도 통과 — 중첩이지만 그 저장소의 git 폴더", path: "/w/a/.claude/worktrees/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/a/.claude/worktrees/x/.git", "gitdir: /w/a/.git/worktrees/x"); f.put("/w/a/.git/worktrees/x/gitdir", "/w/a/.claude/worktrees/x/.git"); f.put("/w/a/.git/worktrees/x/commondir", "../.."); }, top: Some("/w/a/.claude/worktrees/x"), git_dir: Some("/w/a/.git/worktrees/x"), block: None },
        PrepGitRow { note: "따로 작업 트리의 되짚는 자리가 다른 폴더면 막음", path: "/w/wt/y", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/wt/y/.git", "gitdir: /w/a/.git/worktrees/x"); f.put("/w/wt/x/.git", "gitdir: /w/a/.git/worktrees/x"); f.put("/w/a/.git/worktrees/x/gitdir", "/w/wt/x/.git"); f.put("/w/a/.git/worktrees/x/commondir", "../.."); }, top: Some("/w/wt/y"), git_dir: None, block: Some("따로 작업 트리의 되짚는 자리(gitdir)가 이 폴더가 아니라") },
        PrepGitRow { note: "따로 작업 트리의 commondir 가 다른 git 폴더면 막음", path: "/w/wt/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.repo("/w/evil"); f.put("/w/wt/x/.git", "gitdir: /w/a/.git/worktrees/x"); f.put("/w/a/.git/worktrees/x/gitdir", "/w/wt/x/.git"); f.put("/w/a/.git/worktrees/x/commondir", "/w/evil/.git"); }, top: Some("/w/wt/x"), git_dir: None, block: Some("따로 작업 트리의 commondir 가 그 저장소 git 폴더가 아니라") },
        PrepGitRow { note: "⑫-8 꼴 A: sub/.git 이 작업 트리 안 git 폴더를 가리키면 막음", path: "/w/gl/sub", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/gl"); f.repo("/w/gl/.cache/m"); f.put("/w/gl/sub/.git", "gitdir: ../.cache/m/.git\n"); }, top: Some("/w/gl/sub"), git_dir: Some("/w/gl/.cache/m/.git"), block: Some("다른 저장소(/w/gl)의 작업 트리 안에 든 git 자리라") },
        PrepGitRow { note: "⑫-8 꼴 A 의 바깥 저장소(색인에 gitlink): 하위 모듈로 막음", path: "/w/gl", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/gl"); f.repo("/w/gl/.cache/m"); f.put("/w/gl/sub/.git", "gitdir: ../.cache/m/.git"); f.index.insert("/w/gl".to_string(), (vec!["sub".to_string()], false)); }, top: Some("/w/gl"), git_dir: None, block: Some("작업 폴더에 하위 모듈(색인에 중첩 git 저장소(sub))이 있어") },
        PrepGitRow { note: "⑫-8 꼴 B: 하위 폴더 app/.git(중첩 저장소)을 고르면 막음", path: "/w/nest/app", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/nest"); f.repo("/w/nest/app"); }, top: Some("/w/nest/app"), git_dir: None, block: Some("다른 저장소(/w/nest)의 작업 트리 안에 든 git 자리라") },
        PrepGitRow { note: "⑫-8 꼴 B 의 바깥 저장소는 막지 않음(보드 git 이 추적하지 않는 중첩 자리로 들어가지 않음)", path: "/w/nest", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/nest"); f.repo("/w/nest/app"); }, top: Some("/w/nest"), git_dir: Some("/w/nest/.git"), block: None },
        PrepGitRow { note: "꼴 B 에서 하위 폴더 안쪽을 고르면 가까운 app 이 뿌리, 막음", path: "/w/nest/app/src", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/nest"); f.repo("/w/nest/app"); f.mkdir("/w/nest/app/src"); }, top: Some("/w/nest/app"), git_dir: None, block: Some("다른 저장소(/w/nest)의 작업 트리 안에 든 git 자리라") },
        PrepGitRow { note: "홈 폴더 dotfiles 저장소(~/.git)는 보지 않음", path: "/h/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/h"); f.repo("/h/w/a"); }, top: Some("/h/w/a"), git_dir: Some("/h/w/a/.git"), block: None },
        PrepGitRow { note: "홈 밖 저장소도 그 위를 본다", path: "/opt/x/r", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/opt/x"); f.repo("/opt/x/r"); }, top: Some("/opt/x/r"), git_dir: None, block: Some("다른 저장소(/opt/x)의 작업 트리 안에 든 git 자리라") },
        PrepGitRow { note: "하위 모듈 자리(.git/modules) 통과", path: "/w/s/sub", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/s"); f.mkdir("/w/s/.git/modules/sub"); f.put("/w/s/.git/modules/sub/HEAD", ""); f.put("/w/s/sub/.git", "gitdir: ../.git/modules/sub"); }, top: Some("/w/s/sub"), git_dir: Some("/w/s/.git/modules/sub"), block: None },
        // 보안 검토 고침 8(재현: .git/commondir 가 저장소 밖에 심은 공통 git 폴더를 가리키고 그 config 에 clean filter — 옛 확인은 깨끗한 .git/config 만 보고
        // 지나갔고 보드 꼴 git status 가 filter 를 돌림, 표지 2). git 은 commondir 가 가리키는 자리의 config 를 읽는다
        PrepGitRow { note: "보안 검토 고침 8: .git/commondir 가 다른 폴더(심은 config 에 filter)를 가리키면 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/a/.git/commondir", "/w/planted\n"); f.mkdir("/w/planted/objects"); f.mkdir("/w/planted/refs"); f.cfg("/w/planted/config", &[("filter.x.clean", Some("/w/plant.sh"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 폴더(/w/a/.git)에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려") },
        PrepGitRow { note: "보안 검토 고침 8: commondir 가 그 git 폴더 자신을 가리켜도 막음(따로 작업 트리 밖의 commondir 는 받지 않음)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/a/.git/commondir", ".\n"); }, top: Some("/w/a"), git_dir: None, block: Some("git 폴더(/w/a/.git)에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려") },
        PrepGitRow { note: "보안 검토 고침 8: commondir 가 바로 가기여도 있음으로 보고 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.mkdir("/w/planted"); f.links.insert("/w/a/.git/commondir".to_string(), "/w/planted".to_string()); }, top: Some("/w/a"), git_dir: None, block: Some("git 폴더(/w/a/.git)에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려") },
        PrepGitRow { note: "보안 검토 고침 8: 하위 모듈 git 폴더에 commondir 가 있으면 막음", path: "/w/s/sub", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/s"); f.mkdir("/w/s/.git/modules/sub"); f.put("/w/s/.git/modules/sub/HEAD", ""); f.put("/w/s/sub/.git", "gitdir: ../.git/modules/sub"); f.mkdir("/w/planted"); f.put("/w/s/.git/modules/sub/commondir", "/w/planted"); }, top: Some("/w/s/sub"), git_dir: None, block: Some("하위 모듈 git 폴더(/w/s/.git/modules/sub)에 commondir 가 있어 공통 git 폴더를 다른 자리로 돌려") },
        PrepGitRow { note: "하위 모듈 git 폴더가 그 저장소 밖이면 막음", path: "/w/t/sub", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/s"); f.mkdir("/w/s/.git/modules/sub"); f.mkdir("/w/t"); f.put("/w/t/sub/.git", "gitdir: /w/s/.git/modules/sub"); }, top: Some("/w/t/sub"), git_dir: None, block: Some("하위 모듈 git 폴더가 그 저장소 밖을 가리켜") },
        PrepGitRow { note: "보안 검토 고침 2: 떨어진 git 폴더(--separate-git-dir 꼴)는 자리를 믿을 수 없어 막음(전에는 통과)", path: "/w/sep", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.mkdir("/w/store/sep.git"); f.put("/w/store/sep.git/HEAD", ""); f.put("/w/sep/.git", "gitdir: /w/store/sep.git"); }, top: Some("/w/sep"), git_dir: None, block: Some("git 파일이 떨어진 git 폴더(/w/store/sep.git)를 가리켜 자리를 믿을 수 없어") },
        PrepGitRow { note: "보안 검토 고침 1·2: .git 파일이 임시 자리 git 폴더를 가리키면 막음(전에는 통과, 그 설정의 clean filter 를 보드 status 가 돌림)", path: "/w/top", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/tmp/evil"); f.cfg("/tmp/evil/.git/config", &[("filter.x.clean", Some("/w/plant.sh"))]); f.put("/w/top/.git", "gitdir: /tmp/evil/.git"); }, top: Some("/w/top"), git_dir: None, block: Some("git 폴더(/tmp/evil/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어") },
        PrepGitRow { note: "보안 검토 고침 2: 따로 작업 트리의 git 폴더가 임시 자리면 막음", path: "/w/wt/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/private/var/folders/q/m"); f.put("/w/wt/x/.git", "gitdir: /private/var/folders/q/m/.git/worktrees/x"); f.put("/private/var/folders/q/m/.git/worktrees/x/gitdir", "/w/wt/x/.git"); f.put("/private/var/folders/q/m/.git/worktrees/x/commondir", "../.."); }, top: Some("/w/wt/x"), git_dir: None, block: Some("git 폴더(/private/var/folders/q/m/.git/worktrees/x)가 임시 폴더·자율주행 작업 트리 자리에 있어") },
        PrepGitRow { note: "보안 검토 고침 2: 자율주행 작업 트리 자리(~/.claude-board/worktrees) 안 git 폴더면 막음", path: "/w/q", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/h/.claude-board/worktrees/r1/x"); f.put("/w/q/.git", "gitdir: /h/.claude-board/worktrees/r1/x/.git"); }, top: Some("/w/q"), git_dir: None, block: Some("git 폴더(~/.claude-board/worktrees/r1/x/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어") },
        PrepGitRow { note: "보안 검토 고침 2: .git 파일이 그 작업 트리 안 git 폴더를 가리키면 막음", path: "/w/t", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/t/.hide"); f.put("/w/t/.git", "gitdir: .hide/.git"); }, top: Some("/w/t"), git_dir: None, block: Some("git 폴더(/w/t/.hide/.git)가 그 작업 트리 안에 있어") },
        PrepGitRow { note: "보안 검토 고침 1: .git 파일이 다른 곳의 저장소를 가리켜도 고른 폴더가 다른 저장소 작업 트리 안이면 막음(전에는 통과)", path: "/w/x/sub", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/x"); f.repo("/w/other"); f.put("/w/x/sub/.git", "gitdir: /w/other/.git"); }, top: Some("/w/x/sub"), git_dir: None, block: Some("다른 저장소(/w/x)의 작업 트리 안에 든 git 자리라") },
        PrepGitRow { note: "자율주행 작업 트리 자리의 따로 작업 트리(git 폴더는 원래 저장소)는 통과", path: "/h/.claude-board/worktrees/r1/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/h/.claude-board/worktrees/r1/a/.git", "gitdir: /w/a/.git/worktrees/r1"); f.put("/w/a/.git/worktrees/r1/gitdir", "/h/.claude-board/worktrees/r1/a/.git"); f.put("/w/a/.git/worktrees/r1/commondir", "../.."); }, top: Some("/h/.claude-board/worktrees/r1/a"), git_dir: Some("/w/a/.git/worktrees/r1"), block: None },
        PrepGitRow { note: "보안 검토 고침 3: 저장소 설정의 filter 명령(clean)은 막음(보드 status 가 돌림, 전에는 통과)", path: "/w/own", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/own"); f.cfg("/w/own/.git/config", &[("filter.x.clean", Some("/w/plant.sh flt"))]); }, top: Some("/w/own"), git_dir: None, block: Some("저장소 설정(/w/own/.git/config)에 filter 명령(filter.x.clean)이 있어") },
        PrepGitRow { note: "보안 검토 고침 3: smudge·process 도 막음(대소문자 접음)", path: "/w/own", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/own"); f.cfg("/w/own/.git/config", &[("filter.Y.Process", Some("evil"))]); }, top: Some("/w/own"), git_dir: None, block: Some("저장소 설정(/w/own/.git/config)에 filter 명령(filter.y.process)이 있어") },
        PrepGitRow { note: "git-lfs 표준 filter 는 통과", path: "/w/lfs", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/lfs"); f.cfg("/w/lfs/.git/config", &[("filter.lfs.clean", Some("git-lfs clean -- %f")), ("filter.lfs.smudge", Some("git-lfs smudge -- %f")), ("filter.lfs.process", Some("git-lfs filter-process")), ("filter.lfs.required", Some("true"))]); }, top: Some("/w/lfs"), git_dir: Some("/w/lfs/.git"), block: None },
        PrepGitRow { note: "git-lfs 를 흉내 낸 다른 글은 막음", path: "/w/lfs", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/lfs"); f.cfg("/w/lfs/.git/config", &[("filter.lfs.clean", Some("git-lfs clean -- %f; touch x"))]); }, top: Some("/w/lfs"), git_dir: None, block: Some("저장소 설정(/w/lfs/.git/config)에 filter 명령(filter.lfs.clean)이 있어") },
        PrepGitRow { note: "보드가 돌리는 git(ls-files·cat-file·symbolic-ref·for-each-ref·status·rev-list)이 부르지 않는 명령 키는 막지 않음(재현 안 됨)", path: "/w/own", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/own"); f.cfg("/w/own/.git/config", &[("diff.external", Some("x")), ("diff.y.textconv", Some("x")), ("core.pager", Some("x")), ("core.sshcommand", Some("x"))]); }, top: Some("/w/own"), git_dir: Some("/w/own/.git"), block: None },
        PrepGitRow { note: ".git 꼴이 아닌 자리를 가리키면 막음", path: "/w/odd", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.mkdir("/w/store/plain"); f.put("/w/odd/.git", "gitdir: /w/store/plain"); }, top: Some("/w/odd"), git_dir: None, block: Some("git 폴더(/w/store/plain)가 저장소 .git 꼴 자리가 아니라") },
        PrepGitRow { note: "저장소 자체 .git/config 의 core.fsmonitor 명령은 막음", path: "/w/cfg", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/cfg"); f.cfg("/w/cfg/.git/config", &[("core.fsmonitor", Some("/w/cfg/plant.sh"))]); }, top: Some("/w/cfg"), git_dir: None, block: Some("저장소 설정(/w/cfg/.git/config)에 core.fsmonitor 명령(/w/cfg/plant.sh)이 있어") },
        PrepGitRow { note: "core.fsmonitor=true(git 내장 감시)·false 는 통과", path: "/w/cfg", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/cfg"); f.cfg("/w/cfg/.git/config", &[("core.fsmonitor", Some("true")), ("core.fsmonitor", Some("FALSE")), ("core.fsmonitor", None)]); }, top: Some("/w/cfg"), git_dir: Some("/w/cfg/.git"), block: None },
        PrepGitRow { note: "따로 작업 트리의 config.worktree 에 심은 fsmonitor 도 막음", path: "/w/wt/x", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.put("/w/wt/x/.git", "gitdir: /w/a/.git/worktrees/x"); f.put("/w/a/.git/worktrees/x/gitdir", "/w/wt/x/.git"); f.put("/w/a/.git/worktrees/x/commondir", "../.."); f.cfg("/w/a/.git/worktrees/x/config.worktree", &[("core.fsmonitor", Some("touch x"))]); }, top: Some("/w/wt/x"), git_dir: None, block: Some("저장소 설정(/w/a/.git/worktrees/x/config.worktree)에 core.fsmonitor 명령(touch x)이 있어") },
        PrepGitRow { note: "include 가 작업 트리 안을 가리키면 막음(대소문자 접음)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("/W/A/team.gitconfig"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 가 저장소·임시 폴더 안(/W/A/team.gitconfig)을 가리켜") },
        PrepGitRow { note: "상대 include 는 그 파일 폴더 기준(.git 안 = 범위 안)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("includeif.onbranch:main.path", Some("local.cfg"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 가 저장소·임시 폴더 안(/w/a/.git/local.cfg)을 가리켜") },
        PrepGitRow { note: "include 가 임시 폴더를 가리키면 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("/tmp/x.cfg"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 가 저장소·임시 폴더 안(/tmp/x.cfg)을 가리켜") },
        PrepGitRow { note: "범위 밖 include 는 따라가 그 안의 include 도 본다", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("~/team.cfg"))]); f.cfg("/h/team.cfg", &[("include.path", Some("/w/a/evil.cfg"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 가 저장소·임시 폴더 안(/w/a/evil.cfg)을 가리켜") },
        PrepGitRow { note: "범위 밖 파일의 fsmonitor 는 사람 설정이라 막지 않음(보드 git 의 -c 가 덮음)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("~/team.cfg"))]); f.cfg("/h/team.cfg", &[("core.fsmonitor", Some("watchman-hook"))]); }, top: Some("/w/a"), git_dir: Some("/w/a/.git"), block: None },
        PrepGitRow { note: "보안 검토 고침 4: include 로 끌어온 범위 밖 파일의 filter 명령도 막음(보드 status 가 돌림, 전에는 뿌리 파일만 봄)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("~/team.cfg"))]); f.cfg("/h/team.cfg", &[("filter.z.clean", Some("/w/plant.sh"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정(~/team.cfg)에 filter 명령(filter.z.clean)이 있어") },
        PrepGitRow { note: "보안 검토 고침 5: ~<사용자>/ 꼴 include 는 git 이 그 사용자 홈으로 풀어 우리 풀이와 달라 막음(전에는 상대 경로로 읽어 통과)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("~toy/team.cfg"))]); f.cfg("/h/team.cfg", &[("filter.z.clean", Some("/w/plant.sh"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 경로(~toy/team.cfg)를 git 과 같게 풀 수 없어") },
        PrepGitRow { note: "보안 검토 고침 5: %(prefix) 꼴 include 도 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("includeif.gitdir:/w/.path", Some("%(prefix)/etc/x.cfg"))]); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 경로(%(prefix)/etc/x.cfg)를 git 과 같게 풀 수 없어") },
        PrepGitRow { note: "include 가 10단 넘게 겹치면 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.cfg("/w/a/.git/config", &[("include.path", Some("/o/c0"))]); for i in 0..12 { f.cfg(&format!("/o/c{i}"), &[("include.path", Some(&format!("/o/c{}", i + 1)))]); } }, top: Some("/w/a"), git_dir: None, block: Some("git 설정 include 가 10단 넘게 겹쳐") },
        PrepGitRow { note: "설정 파일이 FIFO 면 읽지 못한 것으로 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.fifos.insert("/w/a/.git/config.worktree".to_string()); }, top: Some("/w/a"), git_dir: None, block: Some("git 설정(/w/a/.git/config.worktree)을 읽지 못해") },
        PrepGitRow { note: ".gitmodules 만 있어도 막음", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.index.insert("/w/a".to_string(), (vec![], true)); }, top: Some("/w/a"), git_dir: None, block: Some("작업 폴더에 하위 모듈(.gitmodules 파일)이 있어") },
        PrepGitRow { note: "색인을 못 읽으면 막음(닫힌 쪽)", path: "/w/a", home: "/h", records: vec![], build: |f: &mut FakeGitFS| { f.repo("/w/a"); f.index_fail.insert("/w/a".to_string()); }, top: Some("/w/a"), git_dir: None, block: Some("색인을 읽지 못해(git ls-files 실패)") },
        PrepGitRow { note: "지난 실행 기록과 같은 git 폴더는 통과", path: "/w/a", home: "/h", records: vec![("/w/a", GitRecord { common_dir: Some("/w/a/.git".into()), common_ino: Some("1:/w/a/.git".into()), work_git_dir: Some("/gone".into()), work_git_ino: Some("9:9".into()) })], build: |f: &mut FakeGitFS| { f.repo("/w/a"); }, top: Some("/w/a"), git_dir: Some("/w/a/.git"), block: None },
        PrepGitRow { note: "지난 실행 기록과 inode 가 다르면 막음(바꿔치기)", path: "/w/a", home: "/h", records: vec![("/w/a", GitRecord { common_dir: Some("/w/a/.git".into()), common_ino: Some("1:2".into()), work_git_dir: None, work_git_ino: None })], build: |f: &mut FakeGitFS| { f.repo("/w/a"); }, top: Some("/w/a"), git_dir: None, block: Some("git 폴더가 지난 실행이 출발할 때와 달라져") },
        PrepGitRow { note: "지난 실행 기록의 git 폴더 자리가 다르면 막음", path: "/w/a", home: "/h", records: vec![("/w/a", GitRecord { common_dir: Some("/w/old/.git".into()), common_ino: None, work_git_dir: None, work_git_ino: None })], build: |f: &mut FakeGitFS| { f.repo("/w/a"); }, top: Some("/w/a"), git_dir: None, block: Some("git 폴더가 지난 실행이 출발할 때와 달라져") },
    ]
}

fn check(path: &str, fs: &FakeGitFS, records: &BTreeMap<String, GitRecord>, home: &str) -> PrepGit {
    prep_git_check(path, fs, records, home)
}

#[test]
fn 확인() {
    let mut fails: Vec<String> = Vec::new();
    for r in rows() {
        let mut fs = FakeGitFS::new();
        (r.build)(&mut fs);
        let records: BTreeMap<String, GitRecord> = r.records.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let got = check(r.path, &fs, &records, r.home);
        if got.top.as_deref() != r.top {
            fails.push(format!("{}: top {:?} != {:?}", r.note, got.top, r.top));
        }
        if got.block.as_deref() != r.block {
            fails.push(format!("{}: block {:?} != {:?}", r.note, got.block, r.block));
        }
        if let Some(g) = r.git_dir {
            if got.git_dir.as_deref() != Some(g) {
                fails.push(format!("{}: git_dir {:?} != {g:?}", r.note, got.git_dir));
            }
        }
        if let (None, Some(t), Some(g)) = (r.block, r.top, r.git_dir) {
            let mut want = vec![format!("GIT_DIR={g}"), format!("GIT_COMMON_DIR={}", got.common_dir.as_deref().unwrap_or(g)), format!("GIT_WORK_TREE={t}")];
            want.extend(PREP_GIT_READ_ENV.iter().map(|s| s.to_string()));
            if got.pin() != want {
                fails.push(format!("{}: pin {:?} != {:?}", r.note, got.pin(), want));
            }
        }
        if r.block.is_some() && !got.pin().is_empty() {
            fails.push(format!("{}: 막은 저장소는 박을 env 도 없다", r.note));
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

#[test]
fn 공통폴더박기() {
    let mut f = FakeGitFS::new();
    f.repo("/w/a");
    f.put("/w/wt/x/.git", "gitdir: /w/a/.git/worktrees/x\n");
    f.put("/w/a/.git/worktrees/x/gitdir", "/w/wt/x/.git\n");
    f.put("/w/a/.git/worktrees/x/commondir", "../..\n");
    f.put("/w/a/.git/worktrees/x/HEAD", "");
    let none = BTreeMap::new();
    let wt = prep_git_check("/w/wt/x", &f, &none, "/h");
    assert_eq!(wt.block, None);
    let mut want = strs(&["GIT_DIR=/w/a/.git/worktrees/x", "GIT_COMMON_DIR=/w/a/.git", "GIT_WORK_TREE=/w/wt/x"]);
    want.extend(PREP_GIT_READ_ENV.iter().map(|s| s.to_string()));
    assert_eq!(wt.pin(), want);
    assert_eq!(f.pins.borrow().get("/w/wt/x"), Some(&wt.pin()));
    let plain = prep_git_check("/w/a", &f, &none, "/h");
    let mut want = strs(&["GIT_DIR=/w/a/.git", "GIT_COMMON_DIR=/w/a/.git", "GIT_WORK_TREE=/w/a"]);
    want.extend(PREP_GIT_READ_ENV.iter().map(|s| s.to_string()));
    assert_eq!(plain.pin(), want);
    assert_eq!(f.pins.borrow().get("/w/a"), Some(&plain.pin()));
}

#[test]
fn 읽기env() {
    assert_eq!(PREP_GIT_READ_ENV.to_vec(), vec!["GIT_NO_LAZY_FETCH=1", "GIT_CONFIG_COUNT=1", "GIT_CONFIG_KEY_0=protocol.allow", "GIT_CONFIG_VALUE_0=never"]);
}

#[test]
fn 물려받은env() {
    let inherited = map(&[
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/h"),
        ("GIT_CONFIG_PARAMETERS", "'filter.q.clean'='/w/plant.sh'"),
        ("GIT_CONFIG_COUNT", "2"),
        ("GIT_CONFIG_KEY_0", "core.fsmonitor"),
        ("GIT_CONFIG_VALUE_0", "/w/plant.sh"),
        ("GIT_CONFIG_KEY_1", "x"),
        ("GIT_CONFIG_VALUE_1", "y"),
        ("GIT_SSH_COMMAND", "/w/plant.sh"),
        ("GIT_ALLOW_PROTOCOL", "file"),
        ("GIT_INDEX_FILE", "/tmp/i"),
        ("GIT_DIR", "/elsewhere"),
    ]);
    let pin = PrepGit { top: Some("/w/a".into()), git_dir: Some("/w/a/.git".into()), common_dir: Some("/w/a/.git".into()), block: None }.pin();
    let env = prep_git_env(&inherited, &pin);
    assert_eq!(
        env,
        map(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/h"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_ATTR_NOSYSTEM", "1"),
            ("GIT_NO_REPLACE_OBJECTS", "1"),
            ("GIT_DIR", "/w/a/.git"),
            ("GIT_COMMON_DIR", "/w/a/.git"),
            ("GIT_WORK_TREE", "/w/a"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "protocol.allow"),
            ("GIT_CONFIG_VALUE_0", "never"),
        ])
    );
}

// ---- 추가: 원본 동작 고정(Swift 시험에 없음) ----

#[test]
fn 추가_pin_은_통과한_저장소만_박는다() {
    let ok = PrepGit { top: Some("/w/wt/x".into()), git_dir: Some("/w/a/.git/worktrees/x".into()), common_dir: Some("/w/a/.git".into()), block: None };
    let mut want = strs(&["GIT_DIR=/w/a/.git/worktrees/x", "GIT_COMMON_DIR=/w/a/.git", "GIT_WORK_TREE=/w/wt/x"]);
    want.extend(PREP_GIT_READ_ENV.iter().map(|s| s.to_string()));
    assert_eq!(ok.pin(), want);
    // commonDir 가 없으면 git 폴더를 공통으로 박는다
    let plain = PrepGit { top: Some("/w/a".into()), git_dir: Some("/w/a/.git".into()), ..Default::default() };
    assert_eq!(plain.pin()[1], "GIT_COMMON_DIR=/w/a/.git");
    // 막혔거나 뿌리·git 폴더를 모르면 비어 있다
    assert!(PrepGit { block: Some("까닭".into()), ..ok.clone() }.pin().is_empty());
    assert!(PrepGit { git_dir: None, ..ok.clone() }.pin().is_empty());
    assert!(PrepGit { top: None, ..ok }.pin().is_empty());
    assert!(PrepGit::default().pin().is_empty());
}

#[test]
fn 추가_모양_값() {
    assert_eq!(GitEntry::File(None), GitEntry::File(None));
    assert_ne!(GitEntry::Dir, GitEntry::Link);
    assert_eq!(ConfigEntry::new("core.fsmonitor", None), ConfigEntry { key: "core.fsmonitor".into(), value: None });
    assert_eq!(ConfigEntry::new("a.b", Some("c")).value.as_deref(), Some("c"));
}

#[test]
fn 추가_env_extra_는_첫_등호에서_가르고_등호_없으면_버린다() {
    let env = prep_git_env(&map(&[("PATH", "p")]), &strs(&["GIT_DIR=/a=b", "NOEQ", "=x"]));
    assert_eq!(env.get("GIT_DIR").map(String::as_str), Some("/a=b"));
    assert!(!env.contains_key("NOEQ"));
    assert_eq!(env.get("").map(String::as_str), Some("x"));
    // 박는 값은 보호 값을 덮을 수 있다(원본 순서: 보호 뒤에 extra)
    let over = prep_git_env(&map(&[]), &strs(&["GIT_CONFIG_GLOBAL=/x"]));
    assert_eq!(over.get("GIT_CONFIG_GLOBAL").map(String::as_str), Some("/x"));
}

#[test]
fn 추가_env_git_접두만_뺀다() {
    // GITHUB_TOKEN 은 `GIT_` 접두가 아니다 — 원본도 남긴다
    let env = prep_git_env(&map(&[("GITHUB_TOKEN", "t"), ("GIT", "x"), ("MY_GIT_X", "y"), ("GIT_X", "z")]), &[]);
    assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some("t"));
    assert_eq!(env.get("GIT").map(String::as_str), Some("x"));
    assert_eq!(env.get("MY_GIT_X").map(String::as_str), Some("y"));
    assert!(!env.contains_key("GIT_X"));
}

#[test]
fn 추가_fsmonitor_불리언_꼴() {
    for v in [None, Some(""), Some("true"), Some("FALSE"), Some("Yes"), Some("no"), Some("ON"), Some("off"), Some("1"), Some("0"), Some("  true\t"), Some("\u{3000}off"), Some("\u{a0}1")] {
        assert!(fsmonitor_is_bool(v), "{v:?}");
    }
    for v in ["/w/cfg/plant.sh", "touch x", "watchman-hook", "~/hook", "2", "truee", "tr ue", "false\n", "false\r", "\ntrue", "\u{200b}true"] {
        assert!(!fsmonitor_is_bool(Some(v)), "{v:?}");
    }
}

#[test]
fn 추가_lfs_표준글_표() {
    assert_eq!(lfs_filter("filter.lfs.clean"), Some("git-lfs clean -- %f"));
    assert_eq!(lfs_filter("filter.lfs.smudge"), Some("git-lfs smudge -- %f"));
    assert_eq!(lfs_filter("filter.lfs.process"), Some("git-lfs filter-process"));
    assert_eq!(lfs_filter("filter.lfs.required"), None);
    assert_eq!(lfs_filter("filter.x.clean"), None);
    // 키는 소문자로만 찾는다(git 이 소문자로 주는 키를 받는 쪽이 접는다)
    assert_eq!(lfs_filter("filter.LFS.clean"), None);
}

#[test]
fn 추가_lexical_posix() {
    assert_eq!(lexical("/"), "/");
    assert_eq!(lexical("/w/a"), "/w/a");
    assert_eq!(lexical("/w/a/"), "/w/a");
    assert_eq!(lexical("/w//a/./b"), "/w/a/b");
    assert_eq!(lexical("/w/a/../b"), "/w/b");
    assert_eq!(lexical("/.."), "/");
    assert_eq!(lexical("/w/../../x"), "/x");
    assert_eq!(lexical("//tmp/x"), "/tmp/x");
    assert_eq!(lexical("/h/.claude-board/worktrees/../worktrees"), "/h/.claude-board/worktrees");
    // 대소문자는 접지 않는다(견주는 쪽이 접는다)
    assert_eq!(lexical("/W/A/team.gitconfig"), "/W/A/team.gitconfig");
    // 상대 경로: 작업 폴더로 풀지 않고 정리만 한다
    assert_eq!(lexical("a/./b/../c"), "a/c");
    // POSIX 에서 `\` 는 파일 이름 글자다
    assert_eq!(lexical(r"/w/a\b"), r"/w/a\b");
}

#[test]
fn 추가_lexical_windows() {
    assert_eq!(lexical(r"C:\w\a"), "C:/w/a");
    assert_eq!(lexical("C:/w/a"), "C:/w/a");
    assert_eq!(lexical(r"C:\w\a\"), "C:/w/a");
    assert_eq!(lexical(r"C:\w\a\..\b"), "C:/w/b");
    assert_eq!(lexical(r"C:\w/a\.\b"), "C:/w/a/b");
    assert_eq!(lexical(r"C:\w\\a"), "C:/w/a");
    assert_eq!(lexical(r"C:\"), "C:/");
    assert_eq!(lexical(r"C:\.."), "C:/");
    assert_eq!(lexical(r"C:\w\..\..\x"), "C:/x");
    assert_eq!(lexical("C:"), "C:");
    // 확장 길이·장치 접두
    assert_eq!(lexical(r"\\?\C:\w\a"), "C:/w/a");
    assert_eq!(lexical(r"\\.\C:\w\a"), "C:/w/a");
    assert_eq!(lexical(r"\\?\C:\w\..\x"), "C:/x");
    // UNC
    assert_eq!(lexical(r"\\srv\sh\w\a"), "//srv/sh/w/a");
    assert_eq!(lexical(r"\\srv\sh"), "//srv/sh/");
    assert_eq!(lexical(r"\\srv\sh\..\..\x"), "//srv/sh/x");
    assert_eq!(lexical(r"\\?\UNC\srv\sh\w"), "//srv/sh/w");
    assert_eq!(lexical(r"\\?\unc\srv\sh\w"), "//srv/sh/w");
    // 뿌리 없는 `\x`(현재 드라이브)
    assert_eq!(lexical(r"\w\a"), "/w/a");
    // 윈도가 무시하는 끝 점·공백: 같은 자리가 같은 글이 된다
    assert_eq!(lexical(r"C:\w\a.\b "), "C:/w/a/b");
    assert_eq!(lexical(r"C:\w\a. .\x"), "C:/w/a/x");
    assert_eq!(lexical(r"C:\w\...\x"), "C:/w/x");
    // 견주기: 접두 형이 달라도 소문자로 접으면 같은 자리
    assert_eq!(lexical(r"\\?\c:\W\A").to_lowercase(), lexical(r"C:\w\a").to_lowercase());
    assert_eq!(lexical(r"\\?\UNC\SRV\Sh\W").to_lowercase(), lexical(r"\\srv\sh\w").to_lowercase());
    // 한글·긴 이름이 든 경로도 잘리지 않는다
    assert_eq!(lexical(r"C:\사용자\홍길동\작업"), "C:/사용자/홍길동/작업");
    assert_eq!(lexical(r"\\?\UNC\서버\공유\작업"), "//서버/공유/작업");
}

#[test]
fn 추가_lexical_는_짧거나_깨진_접두에서_죽지_않는다() {
    for p in ["", "\\", "\\\\", r"\\?\", r"\\?\UNC", r"\\?\UNC\", r"\\?\U", r"\\?\한", r"\\?\UN한", r"\\.\", "C:\\\\", "x:", "ab:\\"] {
        let _ = lexical(p);
    }
    assert_eq!(lexical(r"\\?\한글\x"), "/한글/x");
}

#[test]
fn 추가_short_path_posix() {
    assert_eq!(short_path("/h/w/nest/app", "/h"), "~/w/nest/app");
    assert_eq!(short_path("/opt/x", "/h"), "/opt/x");
    assert_eq!(short_path("/h", "/h"), "/h");
    assert_eq!(short_path("/hx/w", "/h"), "/hx/w");
    // 홈이 POSIX 꼴이면 `\` 는 구분자가 아니다
    assert_eq!(short_path(r"/h\x", "/h"), r"/h\x");
    // 대소문자를 가리지 않는 것은 윈도 꼴 홈뿐이다
    assert_eq!(short_path("/H/w", "/h"), "/H/w");
}

#[test]
fn 추가_short_path_windows() {
    let home = r"C:\Users\x";
    assert_eq!(short_path(r"C:\Users\x\w\a", home), r"~\w\a");
    assert_eq!(short_path(r"c:\users\X\w", home), r"~\w");
    assert_eq!(short_path("C:/Users/x/w", home), "~/w");
    assert_eq!(short_path(r"C:\Users\xy\w", home), r"C:\Users\xy\w");
    assert_eq!(short_path(r"C:\Users\x", home), r"C:\Users\x");
    assert_eq!(short_path(r"D:\w", home), r"D:\w");
    assert_eq!(short_path(r"\\srv\sh\w", r"\\srv\sh"), r"~\w");
    // 한글 이름 한가운데에서 자르지 않는다
    assert_eq!(short_path(r"C:\사용자\w", home), r"C:\사용자\w");
}

#[test]
fn 추가_env_윈도는_환경_이름_대소문자를_가리지_않는다() {
    let inherited = map(&[
        ("Path", r"C:\Windows"),
        ("git_config_parameters", "'filter.q.clean'='x'"),
        ("Git_Dir", r"C:\elsewhere"),
        ("GIT_SSH_COMMAND", "x"),
        ("git_ssh", "x"),
        ("USERPROFILE", r"C:\Users\x"),
    ]);
    let env = prep_git_env(&inherited, &[]);
    assert_eq!(
        env,
        map(&[
            ("Path", r"C:\Windows"),
            ("USERPROFILE", r"C:\Users\x"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_ATTR_NOSYSTEM", "1"),
            ("GIT_NO_REPLACE_OBJECTS", "1"),
        ])
    );
}

#[test]
fn 추가_pin_은_윈도_경로를_글자_그대로_박는다() {
    let g = PrepGit { top: Some(r"C:\w\a".into()), git_dir: Some(r"C:\w\a\.git".into()), common_dir: None, block: None };
    let pin = g.pin();
    assert_eq!(&pin[..3], strs(&[r"GIT_DIR=C:\w\a\.git", r"GIT_COMMON_DIR=C:\w\a\.git", r"GIT_WORK_TREE=C:\w\a"]).as_slice());
}

#[test]
fn 글() {
    assert_eq!(
        prep_git_block_text("/h/w/nest/app", "다른 저장소(~/w/nest)의 작업 트리 안에 든 git 자리라", "/h"),
        "보안: ~/w/nest/app 의 다른 저장소(~/w/nest)의 작업 트리 안에 든 git 자리라 그 저장소에서 git 을 돌리지 않았습니다. 사람이 확인한 뒤 다시 읽습니다."
    );
    assert_eq!(short_path("/opt/x", "/h"), "/opt/x");
}

// ---- 추가: prep_git_check 의 Windows 꼴·뿌리·범위 견주기(Swift 시험에 없음) ----

const WHOME: &str = r"C:\Users\x";
const SCOPE_BLOCK: &str = "git 설정 include 가 저장소·임시 폴더 안(";

fn no_records() -> BTreeMap<String, GitRecord> {
    BTreeMap::new()
}

#[test]
fn 추가_뿌리_드라이브에서_멈춘다() {
    let mut f = FakeGitFS::new();
    f.mkdir("C:/w/plain");
    let got = prep_git_check(r"C:\w\plain", &f, &no_records(), WHOME);
    assert_eq!(got, PrepGit::default());
    // 드라이브 뿌리 `C:/` 까지 보고 그 위(`/`·`C:`)는 묻지 않는다
    assert_eq!(*f.entry_calls.borrow(), strs(&["C:/w/plain", "C:/w", "C:/"]));
}

/// 어댑터의 실제 경로가 `\` 꼴(`C:\w\a`·`\\srv\sh\w`)이어도 결과·판정이 `/` 꼴일 때와 같다 — 접두 견주기가 구분자 때문에 빗나가지 않는다
#[test]
fn 추가_어댑터가_역슬래시_실제_경로를_줘도_같다() {
    for bs in [false, true] {
        // 통과: 결과는 `/` 꼴
        let mut f = FakeGitFS::new();
        f.bs = bs;
        f.repo("C:/w/a");
        f.mkdir("C:/w/a/src");
        let got = prep_git_check(r"C:\w\a\src", &f, &no_records(), WHOME);
        assert_eq!((got.top.as_deref(), got.git_dir.as_deref(), got.block.as_deref()), (Some("C:/w/a"), Some("C:/w/a/.git"), None), "bs={bs}");
        // .git 파일이 작업 트리 안 git 폴더를 가리킴
        let mut f = FakeGitFS::new();
        f.bs = bs;
        f.repo("C:/w/t/.hide");
        f.put("C:/w/t/.git", r"gitdir: C:\w\t\.hide\.git");
        let got = prep_git_check(r"C:\w\t", &f, &no_records(), WHOME);
        assert_eq!(got.block.as_deref(), Some("git 폴더(C:/w/t/.hide/.git)가 그 작업 트리 안에 있어"), "bs={bs}");
        // 임시 폴더
        let mut f = FakeGitFS::new();
        f.bs = bs;
        f.repo("C:/Users/x/AppData/Local/Temp/evil");
        f.put("C:/w/top/.git", r"gitdir: C:\Users\x\AppData\Local\Temp\evil\.git");
        let got = prep_git_check(r"C:\w\top", &f, &no_records(), WHOME);
        assert_eq!(got.block.as_deref(), Some("git 폴더(~/AppData/Local/Temp/evil/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어"), "bs={bs}");
        // include 범위
        for v in [r"C:\w\a\evil.cfg", "C:/w/a/evil.cfg", r"c:\W\A\x.cfg"] {
            let mut f = FakeGitFS::new();
            f.bs = bs;
            f.repo("C:/w/a");
            f.cfg("C:/w/a/.git/config", &[("include.path", Some(v))]);
            f.put("C:/w/a/evil.cfg", "");
            let got = prep_git_check(r"C:\w\a", &f, &no_records(), WHOME);
            assert!(got.block.as_deref().is_some_and(|b| b.starts_with(SCOPE_BLOCK)), "bs={bs} {v}: {:?}", got.block);
        }
        // UNC 저장소
        let mut f = FakeGitFS::new();
        f.bs = bs;
        f.repo("//srv/sh/w/a");
        f.cfg("//srv/sh/w/a/.git/config", &[("include.path", Some(r"\\srv\sh\w\a\evil.cfg"))]);
        let got = prep_git_check(r"\\srv\sh\w\a", &f, &no_records(), WHOME);
        assert!(got.block.as_deref().is_some_and(|b| b.starts_with(SCOPE_BLOCK)), "bs={bs}: {:?}", got.block);
        // 따로 작업 트리
        let mut f = FakeGitFS::new();
        f.bs = bs;
        f.repo("C:/w/a");
        f.put("C:/w/wt/x/.git", r"gitdir: C:\w\a\.git\worktrees\x");
        f.put("C:/w/a/.git/worktrees/x/gitdir", r"C:\w\wt\x\.git");
        f.put("C:/w/a/.git/worktrees/x/commondir", r"..\..");
        let got = prep_git_check(r"C:\w\wt\x", &f, &no_records(), WHOME);
        assert_eq!((got.git_dir.as_deref(), got.common_dir.as_deref(), got.block.as_deref()), (Some("C:/w/a/.git/worktrees/x"), Some("C:/w/a/.git"), None), "bs={bs}");
    }
}

#[test]
fn 추가_뿌리가_작업_트리면_그_안의_git_폴더도_작업_트리_안이다() {
    let mut f = FakeGitFS::new();
    f.repo("C:/x");
    f.put("C:/.git", r"gitdir: C:\x\.git");
    let got = prep_git_check(r"C:\", &f, &no_records(), WHOME);
    assert_eq!(got.block.as_deref(), Some("git 폴더(C:/x/.git)가 그 작업 트리 안에 있어"));
}

#[test]
fn 추가_unc_저장소의_따로_작업_트리() {
    let build = |commondir: &str| {
        let mut f = FakeGitFS::new();
        f.repo("//srv/sh/w/a");
        f.put("//srv/sh/w/wt/x/.git", r"gitdir: \\srv\sh\w\a\.git\worktrees\x");
        f.put("//srv/sh/w/a/.git/worktrees/x/gitdir", r"\\srv\sh\w\wt\x\.git");
        f.put("//srv/sh/w/a/.git/worktrees/x/commondir", commondir);
        f
    };
    let ok = prep_git_check(r"\\srv\sh\w\wt\x", &build(r"..\.."), &no_records(), WHOME);
    assert_eq!((ok.git_dir.as_deref(), ok.common_dir.as_deref(), ok.block.as_deref()), (Some("//srv/sh/w/a/.git/worktrees/x"), Some("//srv/sh/w/a/.git"), None));
    let bad = prep_git_check(r"\\srv\sh\w\wt\x", &build(r"..\..\.."), &no_records(), WHOME);
    assert_eq!(bad.block.as_deref(), Some("따로 작업 트리의 commondir 가 그 저장소 git 폴더가 아니라"));
}

/// include 는 10단까지 따라간다: 설정 파일에서 include 10개를 이어 가면 통과, 11개째에서 막는다
#[test]
fn 추가_include_단수_경계() {
    for (n, blocked) in [(10, false), (11, true)] {
        let mut f = FakeGitFS::new();
        f.repo("/w/a");
        f.cfg("/w/a/.git/config", &[("include.path", Some("/o/c0"))]);
        for i in 0..n {
            if i + 1 < n {
                f.cfg(&format!("/o/c{i}"), &[("include.path", Some(&format!("/o/c{}", i + 1)))]);
            } else {
                f.cfg(&format!("/o/c{i}"), &[]);
            }
        }
        let got = prep_git_check("/w/a", &f, &no_records(), "/h");
        assert_eq!(got.block.as_deref(), blocked.then_some("git 설정 include 가 10단 넘게 겹쳐"), "n={n}");
    }
}

#[test]
fn 추가_뿌리_unc_공유에서_멈춘다() {
    let mut f = FakeGitFS::new();
    f.mkdir("//srv/sh/w/plain");
    let got = prep_git_check(r"\\srv\sh\w\plain", &f, &no_records(), WHOME);
    assert_eq!(got, PrepGit::default());
    assert_eq!(*f.entry_calls.borrow(), strs(&["//srv/sh/w/plain", "//srv/sh/w", "//srv/sh/"]));
    // `\\?\UNC\…` 꼴로 줘도 같다
    f.entry_calls.borrow_mut().clear();
    assert_eq!(prep_git_check(r"\\?\UNC\srv\sh\w\plain", &f, &no_records(), WHOME), PrepGit::default());
    assert_eq!(f.entry_calls.borrow().last().map(String::as_str), Some("//srv/sh/"));
}

#[test]
fn 추가_드라이브_뿌리에_저장소가_있으면_통과한다() {
    let mut f = FakeGitFS::new();
    f.repo("C:/");
    f.mkdir("C:/w");
    let got = prep_git_check(r"C:\w", &f, &no_records(), WHOME);
    assert_eq!((got.top.as_deref(), got.git_dir.as_deref(), got.block.as_deref()), (Some("C:/"), Some("C:/.git"), None));
    assert_eq!(got.pin()[2], "GIT_WORK_TREE=C:/");
    // 뿌리가 작업 트리면 그 드라이브 전체가 범위다(`C://` 접두로 견주지 않는다)
    f.cfg("C:/.git/config", &[("include.path", Some(r"C:\anything\x.cfg"))]);
    let got = prep_git_check(r"C:\w", &f, &no_records(), WHOME);
    assert!(got.block.as_deref().is_some_and(|b| b.starts_with(SCOPE_BLOCK)), "{:?}", got.block);
}

#[test]
fn 추가_결과는_슬래시_한_가지다() {
    let mut f = FakeGitFS::new();
    f.repo("C:/w/a");
    f.mkdir("C:/w/a/src");
    let got = prep_git_check(r"C:\w\a\src", &f, &no_records(), WHOME);
    assert_eq!((got.top.as_deref(), got.git_dir.as_deref(), got.common_dir.as_deref(), got.block.as_deref()), (Some("C:/w/a"), Some("C:/w/a/.git"), Some("C:/w/a/.git"), None));
    assert_eq!(got.pin()[0], "GIT_DIR=C:/w/a/.git");
    // 막은 사유 글의 경로도 같은 꼴이고 홈은 `~` 로 줄어든다
    let mut f = FakeGitFS::new();
    f.repo(r"C:/Users/x/nest");
    f.repo(r"C:/Users/x/nest/app");
    let got = prep_git_check(r"C:\Users\x\nest\app", &f, &no_records(), WHOME);
    assert_eq!(got.block.as_deref(), Some("다른 저장소(~/nest)의 작업 트리 안에 든 git 자리라"));
}

#[test]
fn 추가_홈_dotfiles_저장소는_윈도_홈에서도_보지_않는다() {
    let mut f = FakeGitFS::new();
    f.repo("C:/Users/x");
    f.repo("C:/Users/x/w/a");
    let got = prep_git_check(r"C:\Users\x\w\a", &f, &no_records(), WHOME);
    assert_eq!((got.top.as_deref(), got.block.as_deref()), (Some("C:/Users/x/w/a"), None));
    // 홈 밖이면 위 저장소를 본다
    let mut f = FakeGitFS::new();
    f.repo("D:/p");
    f.repo("D:/p/a");
    let got = prep_git_check(r"D:\p\a", &f, &no_records(), WHOME);
    assert_eq!(got.block.as_deref(), Some("다른 저장소(D:/p)의 작업 트리 안에 든 git 자리라"));
}

/// 범위(작업 트리·git 폴더·임시 폴더) 안을 가리키는 include 는 `\`·`/`·접두·대소문자·`..` 꼴이 달라도 모두 막는다
#[test]
fn 추가_범위_안_include_는_경로_꼴과_상관없이_막는다() {
    let inside = [
        r"C:\w\a\evil.cfg",
        "C:/w/a/evil.cfg",
        r"c:\W\A\EVIL.cfg",
        r"C:\w\a\..\a\evil.cfg",
        r"\\?\C:\w\a\evil.cfg",
        "//?/C:/w/a/evil.cfg",
        r"\\.\C:\w\a\evil.cfg",
        r"..\evil.cfg", // .git 폴더 기준 상대 → C:/w/a/evil.cfg
        r"C:\w\a\.git\x.cfg",
        r"C:\Users\x\AppData\Local\Temp\x.cfg",
        "c:/users/X/appdata/local/temp/x.cfg",
        "~/AppData/Local/Temp/x.cfg",
        r"C:\Users\x\.claude-board\worktrees\r1\x.cfg",
        "/tmp/x.cfg",
    ];
    for v in inside {
        let mut f = FakeGitFS::new();
        f.repo("C:/w/a");
        f.cfg("C:/w/a/.git/config", &[("include.path", Some(v))]);
        let got = prep_git_check(r"C:\w\a", &f, &no_records(), WHOME);
        assert!(got.block.as_deref().is_some_and(|b| b.starts_with(SCOPE_BLOCK)), "{v}: {:?}", got.block);
    }
    // 범위 밖은 막지 않는다(접두만 같은 형제 폴더·다른 드라이브·홈 바로 아래)
    for v in [r"C:\w\ab\x.cfg", r"D:\other\team.cfg", r"C:\Users\x\team.cfg", "~/team.cfg", r"..\..\other.cfg", r"\\srv\sh\team.cfg"] {
        let mut f = FakeGitFS::new();
        f.repo("C:/w/a");
        f.cfg("C:/w/a/.git/config", &[("include.path", Some(v))]);
        let got = prep_git_check(r"C:\w\a", &f, &no_records(), WHOME);
        assert_eq!(got.block, None, "{v}");
    }
}

#[test]
fn 추가_범위_안_include_는_unc_저장소에서도_막는다() {
    for v in [r"\\srv\sh\w\a\evil.cfg", "//srv/sh/w/a/evil.cfg", r"\\?\UNC\srv\sh\w\a\evil.cfg", r"\\SRV\SH\W\A\evil.cfg", r"//?/UNC/srv/sh/w/a/evil.cfg"] {
        let mut f = FakeGitFS::new();
        f.repo("//srv/sh/w/a");
        f.cfg("//srv/sh/w/a/.git/config", &[("include.path", Some(v))]);
        let got = prep_git_check(r"\\srv\sh\w\a", &f, &no_records(), WHOME);
        assert!(got.block.as_deref().is_some_and(|b| b.starts_with(SCOPE_BLOCK)), "{v}: {:?}", got.block);
    }
    let mut f = FakeGitFS::new();
    f.repo("//srv/sh/w/a");
    f.cfg("//srv/sh/w/a/.git/config", &[("include.path", Some(r"\\srv\sh\w\ab\x.cfg"))]);
    assert_eq!(prep_git_check(r"\\srv\sh\w\a", &f, &no_records(), WHOME).block, None);
}

/// 절대 경로 `C:\…` 의 include 를 상대 경로로 읽으면 범위를 빠져나가 읽히지 않는 엉뚱한 파일을 본다 — 절대 경로로 읽어 그 안의 filter 도 본다
#[test]
fn 추가_드라이브_절대_include_를_따라가_안의_filter_를_막는다() {
    let mut f = FakeGitFS::new();
    f.repo("C:/w/a");
    f.cfg("C:/w/a/.git/config", &[("include.path", Some(r"D:\team\x.cfg"))]);
    f.cfg("D:/team/x.cfg", &[("filter.z.clean", Some("evil"))]);
    let got = prep_git_check(r"C:\w\a", &f, &no_records(), WHOME);
    assert_eq!(got.block.as_deref(), Some(r"git 설정(D:\team\x.cfg)에 filter 명령(filter.z.clean)이 있어"));
}

#[test]
fn 추가_gitdir_파일의_윈도_꼴() {
    let cases: [(&str, &str); 4] = [
        (r"gitdir: C:\w\t\.hide\.git", "git 폴더(C:/w/t/.hide/.git)가 그 작업 트리 안에 있어"),
        (r"gitdir: .hide\.git", "git 폴더(C:/w/t/.hide/.git)가 그 작업 트리 안에 있어"),
        ("gitdir: C:/w/t/.hide/.git", "git 폴더(C:/w/t/.hide/.git)가 그 작업 트리 안에 있어"),
        (r"gitdir: C:\w\t\nowhere\.git", ".git 파일이 가리키는 git 폴더(C:\\w\\t\\nowhere\\.git)가 없어"),
    ];
    for (text, want) in cases {
        let mut f = FakeGitFS::new();
        f.repo("C:/w/t/.hide");
        f.put("C:/w/t/.git", text);
        let got = prep_git_check(r"C:\w\t", &f, &no_records(), WHOME);
        assert_eq!(got.block.as_deref(), Some(want), "{text}");
    }
}

#[test]
fn 추가_gitdir_파일이_윈도_임시_폴더_작업_트리_자리를_가리키면_막는다() {
    let cases = [
        (r"C:\Users\x\AppData\Local\Temp\evil\.git", "C:/Users/x/AppData/Local/Temp/evil", "git 폴더(~/AppData/Local/Temp/evil/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어"),
        (r"C:\Users\x\.claude-board\worktrees\r1\x\.git", "C:/Users/x/.claude-board/worktrees/r1/x", "git 폴더(~/.claude-board/worktrees/r1/x/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어"),
        // 대소문자: 홈 표기와 실제 폴더 이름의 대소문자가 달라도 같은 자리로 본다
        (r"C:\USERS\X\APPDATA\LOCAL\TEMP\evil\.git", "C:/Users/x/AppData/Local/Temp/evil", "git 폴더(~/AppData/Local/Temp/evil/.git)가 임시 폴더·자율주행 작업 트리 자리에 있어"),
    ];
    for (gitdir, repo, want) in cases {
        for home in [WHOME, r"c:\USERS\x", "C:/Users/x"] {
            let mut f = FakeGitFS::new();
            f.ci = true;
            f.repo(repo);
            f.put("C:/w/top/.git", &format!("gitdir: {gitdir}"));
            let got = prep_git_check(r"C:\w\top", &f, &no_records(), home);
            assert_eq!(got.block.as_deref(), Some(want), "{gitdir} home={home}");
        }
    }
}

#[test]
fn 추가_따로_작업_트리의_윈도_꼴_통과와_거절() {
    let build = |commondir: &str| {
        let mut f = FakeGitFS::new();
        f.repo("C:/w/a");
        f.put("C:/w/wt/x/.git", r"gitdir: C:\w\a\.git\worktrees\x");
        f.put("C:/w/a/.git/worktrees/x/gitdir", "C:/w/wt/x/.git\r\n");
        f.put("C:/w/a/.git/worktrees/x/commondir", commondir);
        f.repo("C:/w/evil");
        f
    };
    let ok = prep_git_check(r"C:\w\wt\x", &build(r"..\.."), &no_records(), WHOME);
    assert_eq!(ok.block, None);
    assert_eq!((ok.git_dir.as_deref(), ok.common_dir.as_deref()), (Some("C:/w/a/.git/worktrees/x"), Some("C:/w/a/.git")));
    let bad = prep_git_check(r"C:\w\wt\x", &build(r"C:\w\evil\.git"), &no_records(), WHOME);
    assert_eq!(bad.block.as_deref(), Some("따로 작업 트리의 commondir 가 그 저장소 git 폴더가 아니라"));
}

#[test]
fn 추가_지난_실행_기록의_키는_꼴과_대소문자를_가리지_않고_찾는다() {
    let rec = GitRecord { common_dir: Some("C:/w/a/.git".into()), common_ino: Some("1:2".into()), work_git_dir: None, work_git_ino: None };
    for key in [r"C:\w\a", "C:/w/a", "c:/W/A", r"\\?\C:\w\a"] {
        let mut f = FakeGitFS::new();
        f.repo("C:/w/a");
        let records: BTreeMap<String, GitRecord> = [(key.to_string(), rec.clone())].into_iter().collect();
        let got = prep_git_check(r"C:\w\a", &f, &records, WHOME);
        assert_eq!(got.block.as_deref(), Some("git 폴더가 지난 실행이 출발할 때와 달라져"), "{key}");
    }
    // 다른 저장소의 기록은 건드리지 않는다
    let mut f = FakeGitFS::new();
    f.repo("C:/w/a");
    let records: BTreeMap<String, GitRecord> = [(r"C:\w\b".to_string(), rec)].into_iter().collect();
    assert_eq!(prep_git_check(r"C:\w\a", &f, &records, WHOME).block, None);
}

#[test]
fn 추가_빈_gitdir_파일과_값_없는_filter_는_원본대로() {
    // `.git` 파일 글이 비면 가리키는 자리가 없다
    let mut f = FakeGitFS::new();
    f.put("/w/g/.git", "gitdir:  \n");
    assert_eq!(prep_git_check("/w/g", &f, &no_records(), "/h").block.as_deref(), Some(".git 파일이 가리키는 git 폴더()가 없어"));
    // 값 없는 filter.x.clean 은 원본의 nil == nil 로 막히지 않는다
    let mut f = FakeGitFS::new();
    f.repo("/w/own");
    f.cfg("/w/own/.git/config", &[("filter.x.clean", None)]);
    assert_eq!(prep_git_check("/w/own", &f, &no_records(), "/h").block, None);
}

#[test]
fn 추가_사유_글은_긴_값을_자른다() {
    let long: String = "가".repeat(70);
    let mut f = FakeGitFS::new();
    f.repo("/w/cfg");
    f.cfg("/w/cfg/.git/config", &[("core.fsmonitor", Some(&long))]);
    let want = format!("저장소 설정(/w/cfg/.git/config)에 core.fsmonitor 명령({}…)이 있어", "가".repeat(60));
    assert_eq!(prep_git_check("/w/cfg", &f, &no_records(), "/h").block, Some(want));
    let long = "나".repeat(90);
    let mut f = FakeGitFS::new();
    f.repo("/w/a");
    f.cfg("/w/a/.git/config", &[("include.path", Some(&format!("~x{long}")))]);
    let want = format!("git 설정 include 경로(~x{}…)를 git 과 같게 풀 수 없어", "나".repeat(78));
    assert_eq!(prep_git_check("/w/a", &f, &no_records(), "/h").block, Some(want));
}
