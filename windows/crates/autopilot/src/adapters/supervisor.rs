//! 다시 띄우기 어댑터 — 작업 스케줄러(`schtasks.exe`)로 `ports::Supervisor` 를 구현한다.
//! macOS LaunchAgent(`KeepAlive{SuccessfulExit=false}`·`RunAtLoad`)의 대응이다. 실행마다 현재 사용자 작업 하나:
//! 로그온 트리거 + 1분 반복, 이미 돌고 있으면 새로 띄우지 않고(`IgnoreNew`), 시간 상한 없이, 관리자 권한 없이.
//! COM 이 아니라 `schtasks.exe` 만 쓴다(windows-sys 에 작업 스케줄러 인터페이스가 없다).
//! 엔진이 죽으면 다음 1분 반복이 다시 띄운다. 둘이 겹쳐 떠도 엔진 잠금이 하나만 남긴다.

use std::path::PathBuf;

use claude_board_core::quote::join_windows;

use crate::ports::{KeepAlive, PortError, PortErrorKind, PortResult, Supervisor};

/// 작업 폴더. 끝 `\` 포함. 작업 경로 = 폴더 + label
pub const FOLDER: &str = r"\ClaudeBoard\autopilot\";
pub const FOLDER_DEV: &str = r"\ClaudeBoard\autopilot-dev\";
/// 시험 전용 — 실제 실행과 섞이지 않게
pub const FOLDER_TEST: &str = r"\ClaudeBoard\autopilot-test\";

/// 오류 글에 싣는 출력 길이(글자 수)
const OUTPUT_LIMIT: usize = 300;

/// schtasks.exe 로 다루는 작업 스케줄러
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskScheduler {
    folder: String,
}

impl TaskScheduler {
    /// dev 면 FOLDER_DEV, 아니면 FOLDER
    pub fn new(dev: bool) -> Self {
        Self { folder: (if dev { FOLDER_DEV } else { FOLDER }).to_string() }
    }

    /// 시험용. `\` 로 시작하고 끝나며 그 사이 조각이 `[A-Za-z0-9._-]+` 인 폴더만(아니면 Invalid)
    pub fn with_folder(folder: &str) -> PortResult<Self> {
        if valid_folder(folder) {
            Ok(Self { folder: folder.to_string() })
        } else {
            Err(PortError::new(PortErrorKind::Invalid, format!("작업 폴더 꼴이 틀렸다: {folder}")))
        }
    }

    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// 폴더 + label. label 이 규칙 밖이면 Invalid
    pub fn task_path(&self, label: &str) -> PortResult<String> {
        if !valid_label(label) {
            return Err(PortError::new(PortErrorKind::Invalid, format!("작업 이름 규칙 밖: {label}")));
        }
        Ok(format!("{}{label}", self.folder))
    }
}

fn valid_folder(folder: &str) -> bool {
    let Some(inner) = folder.strip_prefix('\\').and_then(|f| f.strip_suffix('\\')) else {
        return false;
    };
    !inner.is_empty() && inner.split('\\').all(|part| !part.is_empty() && part.chars().all(label_char))
}

fn label_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// `^[A-Za-z0-9._-]{1,200}$` (macOS label `com.claude.board[.dev].autopilot.<실행 id>` 가 그대로 들어간다)
pub fn valid_label(label: &str) -> bool {
    (1..=200).contains(&label.len()) && label.chars().all(label_char)
}

/// `DOMAIN\NAME`. domain 이 None·빈 글이면 NAME 만, name 이 None·빈 글이면 None
pub fn user_from(domain: Option<&str>, name: Option<&str>) -> Option<String> {
    let name = name.filter(|n| !n.is_empty())?;
    match domain.filter(|d| !d.is_empty()) {
        Some(d) => Some(format!("{d}\\{name}")),
        None => Some(name.to_string()),
    }
}

/// 환경 변수 USERDOMAIN·USERNAME 으로 `user_from`
pub fn current_user() -> Option<String> {
    let domain = std::env::var("USERDOMAIN").ok();
    let name = std::env::var("USERNAME").ok();
    user_from(domain.as_deref(), name.as_deref())
}

/// 지역 시각 `YYYY-MM-DDTHH:MM:SS`(시간대 표기 없음 — 작업 스케줄러가 지역 시각으로 읽는다)
pub fn local_iso(ts: f64) -> String {
    let t = claude_board_core::clock::local(ts);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// XML 글자 이스케이프: `&`→`&amp;`, `<`→`&lt;`, `>`→`&gt;`, `"`→`&quot;`, `'`→`&apos;`
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// 작업 XML 틀. 칸마다 까닭:
/// `Repetition` 에 `Duration` 이 없으면 끝없이 반복한다. `ExecutionTimeLimit PT0S` 는 시간 상한 없음이다(기본 72시간이면 긴 실행의 엔진이 끊긴다).
/// 배터리 두 칸 false 는 노트북에서도 띄우기 위해서다. `InteractiveToken`+`LeastPrivilege` 는 로그인한 사용자 권한으로, 관리자 없이 돌기 위해서다.
/// `LogonTrigger.UserId` 를 비우면 "모든 사용자 로그온"이 되어 관리자가 필요하다.
const TASK_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Claude Board 자율주행 엔진 {label}. 꺼지면 1분 안에 다시 띄운다</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
    <TimeTrigger>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <StartBoundary>{start_boundary}</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{program}</Command>
      <Arguments>{arguments}</Arguments>
      <WorkingDirectory>{working_dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#;

/// 틀의 `{이름}` 을 한 번에 채운다. 채운 값 안의 `{…}` 는 다시 풀지 않는다.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 512);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let hit = after
            .find('}')
            .and_then(|close| values.iter().find(|(k, _)| *k == &after[..close]).map(|(_, v)| (close, *v)));
        match hit {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 작업 XML. 줄 끝 CRLF. Arguments 는 `xml_escape(&join_windows(&job.args))`
pub fn task_xml(job: &KeepAlive, user: &str, start_boundary: &str) -> String {
    let label = xml_escape(&job.label);
    let user = xml_escape(user);
    let start = xml_escape(start_boundary);
    let program = xml_escape(&job.program.to_string_lossy());
    let arguments = xml_escape(&join_windows(&job.args));
    let working_dir = xml_escape(&job.working_dir.to_string_lossy());
    let template = TASK_TEMPLATE.replace('\n', "\r\n");
    fill(
        &template,
        &[
            ("label", &label),
            ("user", &user),
            ("start_boundary", &start),
            ("program", &program),
            ("arguments", &arguments),
            ("working_dir", &working_dir),
        ],
    )
}

/// UTF-16LE + BOM(FF FE). schtasks /xml 은 이 꼴을 읽는다(선언의 encoding="UTF-16" 과 맞춤)
pub fn utf16le_with_bom(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// CSV 줄의 첫 칸(큰따옴표로 감싼 칸, `""` 는 `"`). 따옴표로 시작하지 않거나 닫히지 않으면 None
fn first_csv_cell(line: &str) -> Option<String> {
    let mut chars = line.strip_prefix('"')?.chars().peekable();
    let mut cell = String::new();
    while let Some(c) = chars.next() {
        if c != '"' {
            cell.push(c);
        } else if chars.peek() == Some(&'"') {
            chars.next();
            cell.push('"');
        } else {
            return Some(cell);
        }
    }
    None
}

/// `schtasks /query /fo csv /nh` 출력 → 그 폴더 **바로 아래** 작업 이름(label) 목록. 폴더는 대소문자 무시로 견주고,
/// 하위 폴더 안 작업은 뺀다. 같은 이름은 하나로, 이름 차례로 정렬
pub fn parse_task_list(csv: &str, folder: &str) -> Vec<String> {
    let folder_lower = folder.to_lowercase();
    let folder_len = folder.chars().count();
    let mut names: Vec<String> = csv
        .lines()
        .filter_map(|line| first_csv_cell(line.trim()))
        .filter_map(|cell| {
            let head: String = cell.chars().take(folder_len).collect();
            if head.to_lowercase() != folder_lower {
                return None;
            }
            let name: String = cell.chars().skip(folder_len).collect();
            (!name.is_empty() && !name.contains('\\')).then_some(name)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

// ── schtasks.exe 호출 ────────────────────────────────

fn unavailable() -> PortError {
    PortError::new(PortErrorKind::Unavailable, "작업 스케줄러는 Windows 에서만 쓸 수 있다")
}

/// 종료 코드와 stdout·stderr 를 합친 글. 글은 OEM 코드 페이지라 한글이 깨질 수 있어 판정에 쓰지 않는다.
#[cfg(windows)]
fn run_schtasks(args: &[&str]) -> PortResult<(i32, String)> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// 보드·엔진에서 부를 때 콘솔 창이 깜빡이지 않게
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("schtasks.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("schtasks.exe"));
    let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

#[cfg(not(windows))]
fn run_schtasks(_args: &[&str]) -> PortResult<(i32, String)> {
    Err(unavailable())
}

/// create·delete·run 이 0 이 아닐 때의 오류. 권한 글이 보이면 Denied, 그 밖 Io
fn failure(sub: &str, code: i32, output: &str) -> PortError {
    let lower = output.to_lowercase();
    let denied = ["denied", "access", "거부", "액세스"].iter().any(|w| lower.contains(w));
    let kind = if denied { PortErrorKind::Denied } else { PortErrorKind::Io };
    let shown: String = output.trim().chars().take(OUTPUT_LIMIT).collect();
    PortError::new(kind, format!("schtasks {sub} 실패(종료 코드 {code}): {shown}"))
}

/// 실행해서 0 이 아니면 `failure`
fn run_ok(sub: &str, args: &[&str]) -> PortResult<String> {
    let (code, output) = run_schtasks(args)?;
    if code == 0 {
        Ok(output)
    } else {
        Err(failure(sub, code, &output))
    }
}

#[cfg(windows)]
const IS_WINDOWS: bool = true;
#[cfg(not(windows))]
const IS_WINDOWS: bool = false;

fn ensure_windows() -> PortResult<()> {
    if IS_WINDOWS {
        Ok(())
    } else {
        Err(unavailable())
    }
}

/// `schtasks /create` 인자. `/f`(덮어쓰기)를 일부러 뺐다 — 같은 이름의 작업이 이미 있으면(다른 실행 폴더의 것이거나 지난 실행의 것)
/// 조용히 덮어쓰지 않고 실패한다. 실패한 `register` 는 자기가 만들지 않은 작업을 지우지도 않는다
pub fn create_args(task_path: &str, xml_file: &str) -> Vec<String> {
    ["/create", "/tn", task_path, "/xml", xml_file].map(String::from).to_vec()
}

/// 임시 XML 파일. 떨어지면 지운다(만든 뒤 어느 길로 끝나도)
struct TempXml(PathBuf);

impl Drop for TempXml {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Supervisor for TaskScheduler {
    fn register(&self, job: &KeepAlive) -> PortResult<()> {
        ensure_windows()?;
        let path = self.task_path(&job.label)?;
        let user = current_user()
            .ok_or_else(|| PortError::new(PortErrorKind::Unavailable, "USERNAME 환경 변수가 없어 현재 사용자를 알 수 없다"))?;
        let xml = task_xml(job, &user, &local_iso(claude_board_core::clock::now()));
        let file = std::env::temp_dir().join(format!("cb-task-{}-{}.xml", job.label, std::process::id()));
        // 쓰기가 반쯤 실패해도 파일이 남지 않게, 쓰기 전에 지울 몫부터 쥔다
        let temp = TempXml(file);
        std::fs::write(&temp.0, utf16le_with_bom(&xml))?;
        let args = create_args(&path, &temp.0.to_string_lossy());
        let created = run_ok("create", &args.iter().map(String::as_str).collect::<Vec<_>>());
        drop(temp);
        created?;
        if let Err(e) = run_ok("run", &["/run", "/tn", &path]) {
            let _ = self.unregister(&job.label);
            return Err(e);
        }
        Ok(())
    }

    /// 작업만 지운다. `/end` 는 부르지 않는다 — 엔진이 자기 작업을 지울 때 자기를 끝내지 않게.
    fn unregister(&self, label: &str) -> PortResult<()> {
        ensure_windows()?;
        let path = self.task_path(label)?;
        if !self.is_registered(label)? {
            return Ok(());
        }
        run_ok("delete", &["/delete", "/tn", &path, "/f"]).map(|_| ())
    }

    fn is_registered(&self, label: &str) -> PortResult<bool> {
        ensure_windows()?;
        let path = self.task_path(label)?;
        let (code, _) = run_schtasks(&["/query", "/tn", &path])?;
        Ok(code == 0)
    }

    fn list(&self) -> PortResult<Vec<String>> {
        ensure_windows()?;
        let output = run_ok("query", &["/query", "/fo", "csv", "/nh"])?;
        Ok(parse_task_list(&output, &self.folder))
    }
}
