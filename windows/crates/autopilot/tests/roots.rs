// 출발 때 경로 확인(`cli::roots`) 시험 — 작업 폴더·쓰기 범위·읽기 금지의 정규화와 쓸 수 없는 자리 거르기.
// 실제 디스크의 임시 폴더를 쓰고, 홈·실행 폴더·시스템 폴더는 그 안에 가짜로 세운다(`Forbidden` 의 칸을 직접 채운다).

use std::path::{Path, PathBuf};

use claude_board_autopilot::cli::roots::{check, Forbidden};
use claude_board_autopilot::model::QueueFile;
use claude_board_core::envs::EnvKind;
use serde_json::{json, Value};

struct Fx {
    _tmp: tempfile::TempDir,
    t: PathBuf,
    forbidden: Forbidden,
}

fn canon(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap();
    let s = c.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let t = canon(tmp.path());
        let startup = t.join("home").join("AppData").join("Roaming").join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup");
        for d in ["home/.claude", "runs/r1", "proj", "shared", "sys/Windows", "other"] {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        std::fs::create_dir_all(&startup).unwrap();
        let forbidden = Forbidden {
            home: t.join("home"),
            run_dir: t.join("runs").join("r1"),
            private_dir: t.join("priv").join("r1"),
            system_dirs: vec![t.join("sys").join("Windows")],
            startup,
        };
        Self { _tmp: tmp, t, forbidden }
    }

    fn queue(&self, cwds: &[&Path]) -> QueueFile {
        let units: Vec<Value> = cwds.iter().enumerate().map(|(i, c)| json!({"id": format!("u{i}"), "title": format!("t{i}"), "cwd": c.to_string_lossy(), "check": "x"})).collect();
        serde_json::from_value(Value::Array(units)).unwrap()
    }

    fn run(&self, cwd: &Path, run: Value) -> Result<claude_board_autopilot::cli::roots::Checked, Vec<String>> {
        check(&self.forbidden, EnvKind::Pwsh, &self.queue(&[cwd]), &run)
    }
}

fn first(problems: Vec<String>) -> String {
    problems.into_iter().next().expect("문제가 하나는 있어야 한다")
}

#[test]
fn good_paths_come_back_canonical_and_deduplicated() {
    let fx = Fx::new();
    let shared = fx.t.join("shared");
    let upper = PathBuf::from(shared.to_string_lossy().to_uppercase());
    let secret = fx.t.join("proj").join("secret.txt"); // 없는 파일이어도 읽기 금지는 쓴 대로 둔다
    let run = json!({"write_roots": [shared, upper, fx.t.join("other")], "read_deny": [secret]});
    let got = fx.run(&fx.t.join("proj"), run).unwrap();
    assert_eq!(got.cwds, [fx.t.join("proj")]);
    assert_eq!(got.write_roots, [shared.to_string_lossy().to_string(), fx.t.join("other").to_string_lossy().to_string()], "대소문자만 다른 같은 폴더는 하나");
    assert_eq!(got.read_deny, [secret.to_string_lossy().to_string()]);
    // 설정 칸이 없으면 빈 목록
    let none = fx.run(&fx.t.join("proj"), json!({})).unwrap();
    assert!(none.write_roots.is_empty() && none.read_deny.is_empty());
}

#[test]
fn parent_dir_pieces_are_refused_everywhere() {
    let fx = Fx::new();
    let dotted = |name: &str| PathBuf::from(format!("{}{sep}..{sep}{name}", fx.t.join("other").display(), sep = std::path::MAIN_SEPARATOR));
    let why = first(fx.run(&dotted("proj"), json!({})).unwrap_err());
    assert!(why.contains("단위 1") && why.contains(".."), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"write_roots": [dotted("shared")]})).unwrap_err());
    assert!(why.contains("write_roots") && why.contains(".."), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"read_deny": [dotted("secret")]})).unwrap_err());
    assert!(why.contains("read_deny") && why.contains(".."), "{why}");
}

#[test]
fn network_paths_are_refused_for_native_envs_with_a_plain_message() {
    let fx = Fx::new();
    let unc = Path::new(r"\\server\share\project");
    let why = first(fx.run(unc, json!({})).unwrap_err());
    assert!(why.contains("네트워크 경로") && why.contains("드라이브 글자"), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"write_roots": [r"\\server\share\w"], "read_deny": [r"\\server\share\r"]})).unwrap_err());
    assert!(why.contains("네트워크 경로"), "{why}");
    // WSL 은 `\\wsl.localhost\…` 를 쓰므로 네트워크 꼴 자체는 거부 까닭이 아니다(없는 폴더라서 거부될 뿐)
    let wsl = check(&fx.forbidden, EnvKind::Wsl, &fx.queue(&[unc]), &json!({}));
    assert!(!first(wsl.unwrap_err()).contains("네트워크 경로"));
}

#[test]
fn relative_missing_and_malformed_entries_are_refused() {
    let fx = Fx::new();
    let why = first(fx.run(Path::new(r"rel\dir"), json!({})).unwrap_err());
    assert!(why.contains("절대 경로"), "{why}");
    let why = first(fx.run(&fx.t.join("ghost"), json!({})).unwrap_err());
    assert!(why.contains("단위 1"), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"write_roots": [fx.t.join("ghost")]})).unwrap_err());
    assert!(why.contains("write_roots"), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"write_roots": "C:/x"})).unwrap_err());
    assert!(why.contains("write_roots") && why.contains("글자 배열"), "{why}");
    let why = first(fx.run(&fx.t.join("proj"), json!({"read_deny": [7]})).unwrap_err());
    assert!(why.contains("read_deny") && why.contains("글자 배열"), "{why}");
}

#[test]
fn bad_roots_are_refused_with_the_reason() {
    let fx = Fx::new();
    let f = &fx.forbidden;
    let drive_root = fx.t.ancestors().last().unwrap().to_path_buf();
    let cases: Vec<(PathBuf, &str)> = vec![
        (drive_root, "드라이브 뿌리"),
        (f.home.clone(), "홈 폴더 전체나 그 위"),
        (fx.t.clone(), "홈 폴더 전체나 그 위"),
        (f.home.join(".claude"), ".claude 아래"),
        (f.system_dirs[0].clone(), "시스템 폴더 아래"),
        (f.startup.clone(), "시작 프로그램 폴더와 겹침"),
        (f.run_dir.clone(), "실행 폴더와 겹침"),
        (fx.t.join("runs"), "실행 폴더와 겹침"),
    ];
    for (path, reason) in &cases {
        let why = first(fx.run(path, json!({})).unwrap_err());
        assert!(why.contains(reason), "작업 폴더 {}: {why}", path.display());
        let why = first(fx.run(&fx.t.join("proj"), json!({"write_roots": [path]})).unwrap_err());
        assert!(why.contains("write_roots") && why.contains(reason), "쓰기 범위 {}: {why}", path.display());
    }
    // 엔진 전용 폴더는 아직 없어도 겹침을 본다
    std::fs::create_dir_all(&f.private_dir).unwrap();
    let why = first(fx.run(&f.private_dir, json!({})).unwrap_err());
    assert!(why.contains("엔진 전용 폴더와 겹침"), "{why}");
    // 홈 아래의 다른 폴더는 괜찮다
    std::fs::create_dir_all(f.home.join("work")).unwrap();
    assert!(fx.run(&f.home.join("work"), json!({"write_roots": [f.home.join("work")]})).is_ok());
}

#[test]
fn every_problem_is_collected_not_just_the_first() {
    let fx = Fx::new();
    let run = json!({"write_roots": [fx.forbidden.home, fx.t.join("ghost")], "read_deny": [r"\\s\x"]});
    let problems = fx.run(Path::new(r"rel"), run).unwrap_err();
    assert_eq!(problems.len(), 4, "{problems:?}");
}
