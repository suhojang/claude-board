//! 연결 비밀(API 토큰·webhook 주소) 저장소 어댑터 — 포트 `ports::Secrets`(읽기·쓰기·지우기)와 `rules::connections::ConnSecrets`(읽기만).
//!
//! 원본 `APSecrets` 는 macOS 키체인(서비스 `secret_service(dev)`, 계정 = 연결 id)이다. 여기는 DPAPI 로 봉인한 파일이다.
//! - 비밀 하나 = 파일 하나: `<폴더>/<이름>.dpapi`(내용은 DPAPI 봉인 바이트 그대로). 폴더는 부르는 쪽이 정한다(설치판·시험 빌드는 폴더가 달라
//!   원본 `dev` 가 서비스 이름으로 가르던 것과 같은 구실을 한다). 폴더가 없으면 첫 쓰기에서 `make_private_dir`(현재 사용자만)로 만든다.
//! - 엔트로피: [`ENTROPY`]. 보드 계정 금고(`app/src-tauri/src/accounts.rs` → `win::protect` → `dpapi::protect`)는 엔트로피를 쓰지 않으므로
//!   이 상수가 비어 있지 않은 한 금고의 봉인과 서로 풀리지 않는다(같은 Windows 사용자 안에서 다른 프로그램의 봉인과 섞이지 않게 하는 용도).
//! - 이름: 원본과 같은 `connection_id_ok`(atlassian·notion·figma·webhook-N)만 받는다. 걸러낸 뒤에야 경로를 만들므로 `..`·`\`·`:`·예약 이름이
//!   경로에 닿을 수 없다. 거절 글에는 이름도 비밀도 싣지 않는다.
//! - 쓰기: 같은 폴더 임시 파일 → 디스크까지 내림 → 이름 바꿔 덮기. 실패하면 옛 파일이 그대로 남고 임시 파일은 지운다.
//! - 읽기: 없으면 `None`. 다른 엔트로피로 봉인했거나 망가진 파일은 `Err(Invalid)`(글에 파일 내용·비밀이 없다).
//! - 지우기: 없어도 `Ok`.
//! - 비-Windows: DPAPI 가 `Unsupported` 라 쓰기·읽기가 `Err` 다(평문으로 대신하지 않는다). 지우기·없는 이름 읽기는 파일만 보므로 동작한다.

use crate::ports::{PortError, PortErrorKind, PortResult, RunFiles, Secrets};
use crate::rules::connections::{connection_id_ok, ConnSecrets};
use claude_board_core::dpapi::{protect_with_entropy, unprotect_with_entropy};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 이 저장소 전용 DPAPI 엔트로피. 비밀 파일의 형식 일부이므로 바꾸면 이미 저장한 비밀이 모두 안 풀린다.
pub const ENTROPY: &[u8] = b"claude-board-autopilot/secrets/v1";

const EXT: &str = ".dpapi";
const TEMP_SUFFIX: &str = ".tmp";

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// DPAPI 파일 비밀 저장소. 필드는 폴더 경로뿐이라 `Debug` 에 비밀이 나올 자리가 없다.
#[derive(Debug, Clone)]
pub struct DpapiSecrets {
    dir: PathBuf,
}

impl DpapiSecrets {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 이름을 거른 뒤에만 경로를 만든다.
    fn path_of(&self, name: &str) -> PortResult<PathBuf> {
        if !connection_id_ok(name) {
            return Err(PortError::new(PortErrorKind::Invalid, "비밀 이름 꼴이 아니다"));
        }
        Ok(self.dir.join(format!("{name}{EXT}")))
    }
}

fn sealed_error(op: &str) -> PortError {
    PortError::new(PortErrorKind::Invalid, format!("비밀을 {op} 수 없다(봉인이 다르거나 망가졌거나 이 환경에서 쓸 수 없다)"))
}

impl Secrets for DpapiSecrets {
    fn get(&self, name: &str) -> PortResult<Option<Vec<u8>>> {
        let path = self.path_of(name)?;
        let sealed = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        unprotect_with_entropy(&sealed, ENTROPY).map(Some).map_err(|_| sealed_error("풀"))
    }

    fn put(&self, name: &str, secret: &[u8]) -> PortResult<()> {
        let path = self.path_of(name)?;
        let sealed = protect_with_entropy(secret, ENTROPY).map_err(|_| sealed_error("봉인할"))?;
        if !self.dir.is_dir() {
            crate::adapters::files::FsRunFiles::new().make_private_dir(&self.dir)?;
        }
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = self.dir.join(format!(".{name}.{}.{seq}{TEMP_SUFFIX}", std::process::id()));
        let result = write_temp(&temp, &sealed).and_then(|()| fs::rename(&temp, &path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(PortError::from)
    }

    fn delete(&self, name: &str) -> PortResult<()> {
        let path = self.path_of(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn write_temp(temp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    // 이름을 바꾸기 전에 내용이 디스크에 있어야 정전 뒤에도 빈 파일이 남지 않는다
    file.sync_all()
}

/// 원본 `EngineSecrets` 와 같다: 이름이 틀리면 `None`/`false`, 읽은 글은 끝의 줄바꿈을 깎고 비면 `None`, 읽을 수 없는 파일은 없는 것으로 본다.
impl ConnSecrets for DpapiSecrets {
    fn read(&self, id: &str) -> Option<String> {
        let bytes = self.get(id).ok().flatten()?;
        let text = String::from_utf8(bytes).ok()?;
        let t = text.trim_end_matches(['\n', '\r']);
        (!t.is_empty()).then(|| t.to_string())
    }

    fn exists(&self, id: &str) -> bool {
        matches!(self.get(id), Ok(Some(_)))
    }
}
