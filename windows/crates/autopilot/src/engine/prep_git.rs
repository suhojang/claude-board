//! 사람 저장소의 git 자리 확인을 실제로 돌리는 어댑터와 엔진 진입 함수.
//! 판정은 `rules::prep_git`(순수)가 하고, 여기는 그 규칙이 묻는 답(파일 시스템·git)을 Windows 에서 준다.
//! 원본: `Sources/ClaudeAutopilot/main.swift` 의 `EnginePrepGitWorld`·`prepGit`.
//!
//! 약속:
//! - git 은 모두 `ports::Git`(보호 환경을 거는 `adapters::git::ProcessGit`)로만 부른다. PATH 별칭의 `git` 을 직접 띄우지 않는다.
//! - 설정 파일 읽기(`git config --file …`)는 저장소를 찾지 않는 자리(드라이브 뿌리)에서 돌린다 — 읽을 파일이 든 저장소가 아니라.
//! - 파일 시스템 질문(`entry`·`is_file`·`is_dir`·`text`·`stamp`)은 바로 가기(심볼릭 링크·정션·그 밖의 재분석 지점)를 따라가지 않는다(lstat).
//!   Windows 는 재분석 지점 속성(`FILE_ATTRIBUTE_REPARSE_POINT`)이 있으면 모두 바로 가기로 본다(닫힌 쪽).
//! - "장치:inode" 자리는 Windows 에서 "볼륨 일련번호:파일 인덱스"(`GetFileInformationByHandle`, 바로 가기를 열지 않고)다. 둘 다 16진수.
//! - 박는 env(`PrepGit::pin`)는 `ports::Git` 으로 환경을 못 넘기므로 그중 `GIT_DIR`·`GIT_WORK_TREE` 만 `--git-dir`·`--work-tree` 로 바꿔 건다.
//!   `GIT_COMMON_DIR` 는 명령줄 대응이 없어 못 건다(미해결, 보고서에 적음). 늦은 받기 끔·전송 프로토콜 막음은 `ProcessGit` 이 이미 건다.

use crate::adapters::git::ProcessGit;
use crate::ports::Git;
use crate::rules::board_git::GitRecord;
use crate::rules::gitlinks::gitlink_paths;
use crate::rules::prep_git::{prep_git_block_text, prep_git_check, ConfigEntry, GitEntry, PrepGit, PrepGitWorld};
use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `text` 가 읽는 파일 크기 상한(원본과 같다)
const TEXT_LIMIT: u64 = 65_536;

/// 파일 시스템 종류(따라가지 않고 본 것)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Link,
    Other,
}

#[cfg(windows)]
fn is_reparse(m: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
}

#[cfg(not(windows))]
fn is_reparse(_m: &Metadata) -> bool {
    false
}

fn kind_of(m: &Metadata) -> Kind {
    let ft = m.file_type();
    if ft.is_symlink() || is_reparse(m) {
        Kind::Link
    } else if ft.is_dir() {
        Kind::Dir
    } else if ft.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

fn kind(path: &str) -> Option<Kind> {
    std::fs::symlink_metadata(path).ok().map(|m| kind_of(&m))
}

/// 볼륨 일련번호와 64비트 파일 인덱스(위·아래 32비트)를 "볼륨:인덱스"(16진수)로 적는다
pub fn format_file_id(volume: u32, index_high: u32, index_low: u32) -> String {
    format!("{:x}:{:x}", volume, (u64::from(index_high) << 32) | u64::from(index_low))
}

/// "볼륨 일련번호:파일 인덱스"(Windows) 또는 "장치:inode"(그 밖). 바로 가기는 따라가지 않는다. 없으면 None
#[cfg(windows)]
pub fn file_id(path: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: wide 는 NUL 로 끝나는 살아 있는 버퍼이고, 핸들은 이 함수 안에서 열고 닫는다. info 는 0 으로 채운 값이다.
    unsafe {
        let h = CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let ok = GetFileInformationByHandle(h, &mut info);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(format_file_id(info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    }
}

#[cfg(not(windows))]
pub fn file_id(path: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| format!("{:x}:{:x}", m.dev(), m.ino()))
}

/// 저장소를 찾지 않는 자리(`git config --file` 을 돌릴 폴더) — 드라이브 뿌리
fn neutral_cwd() -> PathBuf {
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").ok().filter(|d| d.len() == 2 && d.ends_with(':')).unwrap_or_else(|| "C:".to_string());
        PathBuf::from(format!("{drive}\\"))
    } else {
        PathBuf::from("/")
    }
}

/// 실제 파일 시스템과 git 으로 답하는 `PrepGitWorld`
pub struct RealPrepGitWorld<'a> {
    git: &'a dyn Git,
    cwd: PathBuf,
}

impl<'a> RealPrepGitWorld<'a> {
    pub fn new(git: &'a dyn Git) -> Self {
        RealPrepGitWorld { git, cwd: neutral_cwd() }
    }

    fn git_out(&self, repo: &Path, args: &[&str]) -> Option<String> {
        let out = self.git.run(repo, args).ok()?;
        (out.code == 0).then_some(out.stdout)
    }
}

impl PrepGitWorld for RealPrepGitWorld<'_> {
    fn entry(&self, dir: &str) -> Option<GitEntry> {
        let p = format!("{}/.git", dir.trim_end_matches('/'));
        match kind(&p)? {
            Kind::Dir => Some(GitEntry::Dir),
            Kind::Link => Some(GitEntry::Link),
            Kind::File => Some(match self.text(&p).as_deref().and_then(|t| t.strip_prefix("gitdir:")) {
                Some(rest) => GitEntry::File(Some(rest.to_string())),
                None => GitEntry::File(None),
            }),
            Kind::Other => Some(GitEntry::Other),
        }
    }

    fn real(&self, path: &str) -> Option<String> {
        std::fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
    }

    fn text(&self, path: &str) -> Option<String> {
        let m = std::fs::symlink_metadata(path).ok()?;
        if kind_of(&m) != Kind::File || m.len() > TEXT_LIMIT {
            return None;
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT — 열 때 바꿔치기된 바로 가기를 따라가지 않는다
        }
        let f = opts.open(path).ok()?;
        // 연 핸들로 다시 확인한다(그 사이 바뀐 자리)
        let hm = f.metadata().ok()?;
        if kind_of(&hm) != Kind::File {
            return None;
        }
        let mut buf = Vec::new();
        f.take(TEXT_LIMIT).read_to_end(&mut buf).ok()?;
        String::from_utf8(buf).ok()
    }

    fn is_file(&self, path: &str) -> bool {
        kind(path) == Some(Kind::File)
    }

    fn is_dir(&self, path: &str) -> bool {
        kind(path) == Some(Kind::Dir)
    }

    fn stamp(&self, path: &str) -> Option<String> {
        file_id(path)
    }

    fn config(&self, file: &str) -> Option<Vec<ConfigEntry>> {
        match kind(file) {
            None => return Some(Vec::new()),
            Some(Kind::File) => {}
            Some(_) => return None,
        }
        let out = self.git_out(&self.cwd, &["config", "--file", file, "--no-includes", "--null", "--list"])?;
        Some(parse_config_list(&out))
    }

    fn submodules(&self, top: &str, pin: &[String]) -> Option<(Vec<String>, bool)> {
        let pinned = |name: &str| pin.iter().find_map(|s| s.strip_prefix(name).and_then(|r| r.strip_prefix('=')));
        // 확인한 git 폴더를 모르면 git 이 스스로 찾게 두지 않고 닫는다
        let gd = format!("--git-dir={}", pinned("GIT_DIR")?);
        let wt = format!("--work-tree={}", pinned("GIT_WORK_TREE")?);
        let repo = Path::new(top);
        let run = |args: &[&str]| -> Option<String> {
            let mut a: Vec<&str> = vec![gd.as_str(), wt.as_str()];
            a.extend_from_slice(args);
            self.git_out(repo, &a)
        };
        let ls = run(&["-c", "core.quotePath=false", "ls-files", "-s", "-z"])?;
        let gitmodules = kind(&format!("{}/.gitmodules", top.trim_end_matches('/'))).is_some()
            || !run(&["ls-files", "-s", "-z", "--", ".gitmodules"]).unwrap_or_default().is_empty()
            || !run(&["ls-tree", "HEAD", "--", ".gitmodules"]).unwrap_or_default().is_empty();
        Some((gitlink_paths(&ls), gitmodules))
    }
}

/// `git config --null --list` 출력(`키\n값\0` 꼴, 값 없는 키는 `키\0`) → 항목들
pub fn parse_config_list(out: &str) -> Vec<ConfigEntry> {
    out.split('\0')
        .filter(|e| !e.is_empty())
        .map(|e| match e.split_once('\n') {
            Some((k, v)) => ConfigEntry { key: k.to_string(), value: Some(v.to_string()) },
            None => ConfigEntry { key: e.to_string(), value: None },
        })
        .collect()
}

/// 엔진 진입: 경로 하나의 저장소와 막을 까닭(규칙 `prep_git_check` 을 실제 세계로 돌린다). `records` 는 이어 가는 지난 실행의 git 자리 기록.
pub fn run_prep_git(git: &dyn Git, path: &str, records: &BTreeMap<String, GitRecord>, home: &str) -> PrepGit {
    prep_git_check(path, &RealPrepGitWorld::new(git), records, home)
}

/// 기본 길: 보호 환경 `ProcessGit`·사용자 홈·기록 없음. 출발 전 확인(`start`)과 보드가 같은 함수를 부른다.
pub fn check_path(path: &str) -> PrepGit {
    let home = claude_board_core::paths::home();
    run_prep_git(&ProcessGit::new(), path, &BTreeMap::new(), &home.to_string_lossy())
}

/// 막혔으면 사용자에게 보일 글(`보안: <저장소> 의 <까닭> …`), 통과했거나 저장소가 아니면 None
pub fn block_message(g: &PrepGit, home: &str) -> Option<String> {
    Some(prep_git_block_text(g.top.as_deref()?, g.block.as_deref()?, home))
}
