//! 앱 쪽 md-render — 답변 꾸미기 플러그인(`plugins/md-render`)을 `%USERPROFILE%\.claude\notifier\plugins\md-render\` 로 옮기고,
//! 보드 탭에서 띄우는 claude 에 `--plugin-dir` 을 붙일 때 그 경로를 프론트에 알려 준다(mac `mdRenderDir`).
//!
//! 설치 폴더(`ClaudeBoard.exe` 옆) 사본을 그대로 넘기지 않고 옮겨 쓰는 까닭은 대화형 claude 가 플러그인 폴더에
//! `tsconfig.json`·`.claude-plugin\types\` 를 쓰기 때문이다. 업데이트가 설치 폴더를 바꿔 끼워도 경로가 유지된다.
//! 옮기는 것은 `PLUGIN_FILES` 다섯 개뿐이고, 내용(sha256)이 다른 것만 덮으며 claude 가 만든 파일은 건드리지 않는다.
//! 옮길 폴더·하위 폴더·파일이 심볼릭 링크나 정션이면 따라 쓰지 않고 거절한다(링크 너머의 다른 폴더를 덮지 않으려는 것이다).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use claude_board_core::paths;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::board::Board;
use crate::settings::set_in;
use crate::util::board_log;

/// 플러그인에 싣는 다섯 파일(저장소 `plugins/md-render` 기준 상대 경로, `/` 구분) — `tests/` 는 싣지 않는다.
pub const PLUGIN_FILES: [&str; 5] = ["LICENSE", "README.md", ".claude-plugin/plugin.json", "hooks/hooks.json", "hooks/register.tsx"];

/// (상대 경로, sha256 16진수) 목록 — 폴더 하나에 든 `PLUGIN_FILES`.
pub type Listing = Vec<(String, String)>;

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn file_in(root: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part))
}

/// 폴더에 있는 `PLUGIN_FILES` 의 해시 — 없는 파일은 목록에 없다. 파일이 아닌 것(폴더)이 그 자리에 있어도 없는 것으로 본다.
pub fn listing(root: &Path) -> Listing {
    PLUGIN_FILES
        .iter()
        .filter_map(|relative| fs::read(file_in(root, relative)).ok().map(|bytes| ((*relative).to_string(), hex_sha256(&bytes))))
        .collect()
}

/// 옮길 파일 — source 에 있고 destination 에 없거나 해시가 다른 것(source 의 차례대로).
pub fn copy_plan(source: &[(String, String)], destination: &[(String, String)]) -> Vec<String> {
    source
        .iter()
        .filter(|(relative, hash)| !destination.iter().any(|(there, same)| there == relative && same == hash))
        .map(|(relative, _)| relative.clone())
        .collect()
}

/// 심볼릭 링크나 정션(재해석 지점)인가 — 없는 경로는 아니다.
fn is_link(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else { return false };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// destination 과 그 안에서 쓸 하위 폴더·파일 가운데 링크가 하나라도 있으면 그 경로. 없으면 None.
fn first_link(destination: &Path, relatives: &[String]) -> Option<PathBuf> {
    let mut candidates = vec![destination.to_path_buf()];
    for relative in relatives {
        let file = file_in(destination, relative);
        let mut folders: Vec<PathBuf> = file.ancestors().skip(1).take_while(|folder| folder.starts_with(destination) && *folder != destination).map(Path::to_path_buf).collect();
        folders.reverse();
        candidates.extend(folders);
        candidates.push(file);
    }
    candidates.into_iter().find(|path| is_link(path))
}

/// source 의 다섯 파일 가운데 내용이 다른 것만 destination 으로 옮기고 옮긴 경로를 돌려준다.
/// source 에 다섯 중 하나라도 없으면(깨진 설치) 아무것도 옮기지 않고 NotFound 로 실패한다. destination 의 그 밖 파일은 건드리지 않는다.
/// destination·하위 폴더·옮길 파일이 링크(심볼릭 링크·정션)이면 아무것도 쓰지 않고 PermissionDenied 로 실패한다.
pub fn sync(source: &Path, destination: &Path) -> io::Result<Vec<PathBuf>> {
    let wanted = listing(source);
    if let Some(missing) = PLUGIN_FILES.iter().find(|relative| !wanted.iter().any(|(there, _)| there == **relative)) {
        let at = file_in(source, missing);
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} 이 없습니다", at.display())));
    }
    let plan = copy_plan(&wanted, &listing(destination));
    if let Some(link) = first_link(destination, &plan) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} 는 링크라 따라 쓰지 않습니다", link.display())));
    }
    let mut copied = Vec::new();
    for relative in plan {
        let to = file_in(destination, &relative);
        let bytes = fs::read(file_in(source, &relative))?;
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        // 쓰다 끊겨도 반쪽 파일이 남지 않게 옆 파일에 쓰고 바꿔 끼운다
        let partial = to.with_extension("cb-partial");
        fs::write(&partial, bytes)?;
        if let Err(e) = fs::rename(&partial, &to) {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        copied.push(to);
    }
    Ok(copied)
}

/// 설치 폴더의 사본 — `ClaudeBoard.exe` 옆 `plugins\md-render`(Tauri `bundle.resources`).
fn installed_source() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let folder = exe.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "실행 파일의 폴더를 알 수 없습니다"))?;
    Ok(folder.join("plugins").join("md-render"))
}

/// 옮겨 둘 자리 — `<notifier>\plugins\md-render`.
fn user_copy() -> PathBuf {
    paths::notifier_dir().join("plugins").join("md-render")
}

/// 옮긴 결과를 쥔다 — 성공만 쥐고 실패는 쥐지 않아서, 파일이 잠겼던 것처럼 한때의 실패는 다음 호출이 다시 옮겨 본다.
struct MovedCache(Mutex<Option<PathBuf>>);

impl MovedCache {
    const fn new() -> Self {
        MovedCache(Mutex::new(None))
    }

    /// 쥔 값이 있으면 그것, 없으면 `attempt` 를 한 번 해 보고 성공하면 쥔다. 실패는 board.log 에 남기고 None.
    fn get_or_try(&self, attempt: impl FnOnce() -> io::Result<PathBuf>) -> Option<PathBuf> {
        let mut held = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if held.is_none() {
            match attempt() {
                Ok(destination) => *held = Some(destination),
                Err(e) => board_log(&format!("답변 꾸미기 플러그인을 옮기지 못했습니다: {e}")),
            }
        }
        held.clone()
    }
}

/// 처음 부를 때 옮기고 성공하면 그 결과를 쥔다(mac `mdRenderDir` 의 `let`). 옮기지 못하면 None 이고 이유는 board.log 에 남기며, 다음 호출이 다시 시도한다.
fn moved_once() -> Option<PathBuf> {
    static MOVED: MovedCache = MovedCache::new();
    MOVED.get_or_try(|| {
        let destination = user_copy();
        installed_source().and_then(|source| sync(&source, &destination)).map(|_| destination)
    })
}

/// 프론트의 `mdRenderDir()` — 옮겨 둔 md-render 폴더. 옮기지 못했으면 None(그러면 `--plugin-dir` 을 붙이지 않는다).
#[tauri::command(async)]
pub fn md_render_dir() -> Option<String> {
    moved_once().map(|dir| dir.to_string_lossy().into_owned())
}

/// 프론트의 `rememberMdRenderOff(id)` — 꾸미기를 끄고 띄운 세션 id 를 설정(`mdRenderOff`)에 넣는다.
#[tauri::command(async)]
pub fn remember_md_render_off(board: State<'_, Arc<Board>>, id: String) {
    board.update_settings(|s| set_in(&mut s.md_render_off, &id, true));
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("cb-mdrender-{}", crate::util::short_id()));
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(root: &Path, relative: &str, text: &str) {
        let at = file_in(root, relative);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, text).unwrap();
    }

    fn fill_source(root: &Path) {
        for relative in PLUGIN_FILES {
            write(root, relative, &format!("원본 {relative}"));
        }
    }

    fn entry(relative: &str, hash: &str) -> (String, String) {
        (relative.to_string(), hash.to_string())
    }

    // ── copy_plan 표 ──

    #[test]
    fn copy_plan_table() {
        let a = entry("LICENSE", "1");
        let b = entry("README.md", "2");
        let b_changed = entry("README.md", "9");
        // (원본, 옮겨 둔 것, 기대 — 옮길 상대 경로)
        let table: Vec<(Listing, Listing, Vec<&str>)> = vec![
            (vec![], vec![], vec![]),
            (vec![a.clone(), b.clone()], vec![a.clone(), b.clone()], vec![]),
            (vec![a.clone(), b.clone()], vec![], vec!["LICENSE", "README.md"]),
            (vec![a.clone(), b.clone()], vec![a.clone(), b_changed.clone()], vec!["README.md"]),
            (vec![a.clone()], vec![a.clone(), b_changed], vec![]),
            (vec![b.clone(), a.clone()], vec![entry("LICENSE", "2")], vec!["README.md", "LICENSE"]),
        ];
        for (index, (source, destination, expected)) in table.iter().enumerate() {
            assert_eq!(copy_plan(source, destination), *expected, "행 {index}");
        }
    }

    // ── sync 표 ──

    #[test]
    fn missing_destination_is_created_with_all_five_files() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        let copied = sync(&scratch.path("src"), &scratch.path("dst").join("plugins").join("md-render")).unwrap();
        assert_eq!(copied.len(), 5);
        for relative in PLUGIN_FILES {
            let there = file_in(&scratch.path("dst").join("plugins").join("md-render"), relative);
            assert_eq!(fs::read_to_string(there).unwrap(), format!("원본 {relative}"));
        }
    }

    #[test]
    fn unchanged_files_are_not_copied_again() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        assert_eq!(sync(&scratch.path("src"), &scratch.path("dst")).unwrap().len(), 5);
        let marker = file_in(&scratch.path("dst"), "hooks/hooks.json");
        let before = fs::metadata(&marker).unwrap().modified().unwrap();
        assert!(sync(&scratch.path("src"), &scratch.path("dst")).unwrap().is_empty());
        assert_eq!(fs::metadata(&marker).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn only_changed_file_is_overwritten() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        write(&scratch.path("src"), "hooks/register.tsx", "새 판");
        let copied = sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        assert_eq!(copied, vec![file_in(&scratch.path("dst"), "hooks/register.tsx")]);
        assert_eq!(fs::read_to_string(file_in(&scratch.path("dst"), "hooks/register.tsx")).unwrap(), "새 판");
    }

    #[test]
    fn edited_copy_is_restored_from_source() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        write(&scratch.path("dst"), "README.md", "누가 고침");
        assert_eq!(sync(&scratch.path("src"), &scratch.path("dst")).unwrap().len(), 1);
        assert_eq!(fs::read_to_string(file_in(&scratch.path("dst"), "README.md")).unwrap(), "원본 README.md");
    }

    #[test]
    fn files_claude_wrote_are_left_alone() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        write(&scratch.path("dst"), "tsconfig.json", "{\"claude\":1}");
        write(&scratch.path("dst"), ".claude-plugin/types/index.d.ts", "declare const x: 1;");
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        write(&scratch.path("src"), "LICENSE", "바뀐 라이선스");
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        assert_eq!(fs::read_to_string(file_in(&scratch.path("dst"), "tsconfig.json")).unwrap(), "{\"claude\":1}");
        assert_eq!(fs::read_to_string(file_in(&scratch.path("dst"), ".claude-plugin/types/index.d.ts")).unwrap(), "declare const x: 1;");
        assert_eq!(fs::read_to_string(file_in(&scratch.path("dst"), "LICENSE")).unwrap(), "바뀐 라이선스");
    }

    #[test]
    fn source_files_outside_the_five_are_not_copied() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        write(&scratch.path("src"), "tests/plugin.test.tsx", "시험");
        write(&scratch.path("src"), "tsconfig.json", "{}");
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        assert!(!file_in(&scratch.path("dst"), "tests/plugin.test.tsx").exists());
        assert!(!file_in(&scratch.path("dst"), "tsconfig.json").exists());
    }

    #[test]
    fn broken_source_fails_without_copying_anything() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        fs::remove_file(file_in(&scratch.path("src"), "hooks/register.tsx")).unwrap();
        let error = sync(&scratch.path("src"), &scratch.path("dst")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("register.tsx"));
        assert!(!scratch.path("dst").exists());
        // 원본 폴더가 아예 없을 때도 같다
        assert_eq!(sync(&scratch.path("none"), &scratch.path("dst")).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn blocked_destination_reports_the_failure() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        // 옮길 자리에 같은 이름의 폴더가 있으면 옮기지 못하고 실패한다(그러면 md_render_dir 는 None)
        fs::create_dir_all(file_in(&scratch.path("dst"), "LICENSE")).unwrap();
        assert!(sync(&scratch.path("src"), &scratch.path("dst")).is_err());
    }

    // ── 링크·정션 ──

    // 정션은 권한 없이 만들 수 있다(`mklink /J`). Windows 가 아니면 만들 수 없어 시험을 건너뛴다.
    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) {
        let done = std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().unwrap();
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stdout));
    }

    #[cfg(windows)]
    #[test]
    fn junction_destination_is_refused_and_target_is_untouched() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        write(&scratch.path("elsewhere"), "LICENSE", "다른 폴더의 파일");
        let destination = scratch.path("plugins").join("md-render");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        make_junction(&destination, &scratch.path("elsewhere"));
        let error = sync(&scratch.path("src"), &destination).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert_eq!(fs::read_to_string(scratch.path("elsewhere").join("LICENSE")).unwrap(), "다른 폴더의 파일");
        assert!(!scratch.path("elsewhere").join("hooks").exists(), "링크 너머에 아무것도 쓰지 않는다");
    }

    #[cfg(windows)]
    #[test]
    fn junction_subfolder_is_refused() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        fs::create_dir_all(scratch.path("elsewhere")).unwrap();
        let destination = scratch.path("dst");
        fs::create_dir_all(&destination).unwrap();
        make_junction(&destination.join("hooks"), &scratch.path("elsewhere"));
        let error = sync(&scratch.path("src"), &destination).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert_eq!(fs::read_dir(scratch.path("elsewhere")).unwrap().count(), 0, "링크 너머에 쓰지 않는다");
        assert!(!destination.join("LICENSE").exists(), "링크를 만나면 일부도 옮기지 않는다");
    }

    #[cfg(windows)]
    #[test]
    fn symlinked_file_is_refused_when_symlinks_can_be_made() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        write(&scratch.path("elsewhere"), "LICENSE", "다른 폴더의 파일");
        fs::create_dir_all(scratch.path("dst")).unwrap();
        if std::os::windows::fs::symlink_file(scratch.path("elsewhere").join("LICENSE"), scratch.path("dst").join("LICENSE")).is_err() {
            eprintln!("이 계정은 심볼릭 링크를 만들 수 없어 건너뛴다(정션 시험이 같은 길을 본다)");
            return;
        }
        let error = sync(&scratch.path("src"), &scratch.path("dst")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        assert_eq!(fs::read_to_string(scratch.path("elsewhere").join("LICENSE")).unwrap(), "다른 폴더의 파일");
    }

    #[test]
    fn plain_folders_are_not_links() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        sync(&scratch.path("src"), &scratch.path("dst")).unwrap();
        assert!(!is_link(&scratch.path("dst")));
        assert!(!is_link(&scratch.path("none")));
        assert!(first_link(&scratch.path("dst"), &PLUGIN_FILES.map(String::from)).is_none());
    }

    // ── 실패는 쥐지 않는다 ──

    #[test]
    fn failure_is_not_cached_so_the_next_call_retries() {
        let cache = MovedCache::new();
        let mut calls = 0;
        assert_eq!(cache.get_or_try(|| { calls += 1; Err(io::Error::other("잠김")) }), None);
        let held = PathBuf::from("md-render");
        assert_eq!(cache.get_or_try(|| { calls += 1; Ok(held.clone()) }), Some(held.clone()));
        assert_eq!(cache.get_or_try(|| { calls += 1; Err(io::Error::other("다시 안 부른다")) }), Some(held));
        assert_eq!(calls, 2, "성공을 쥔 뒤에는 시도하지 않는다");
    }

    #[test]
    fn korean_and_spaced_destination_works() {
        let scratch = Scratch::new();
        fill_source(&scratch.path("src"));
        let destination = scratch.path("한글 폴더").join("md render");
        assert_eq!(sync(&scratch.path("src"), &destination).unwrap().len(), 5);
        assert_eq!(listing(&destination).len(), 5);
    }

    #[test]
    fn listing_hashes_content() {
        let scratch = Scratch::new();
        write(&scratch.path("a"), "LICENSE", "abc");
        // sha256("abc")
        assert_eq!(listing(&scratch.path("a")), vec![entry("LICENSE", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")]);
        assert!(listing(&scratch.path("empty")).is_empty());
    }

    #[test]
    fn plugin_files_match_the_repository_plugin() {
        // 저장소의 plugins/md-render 에 다섯 파일이 모두 있어야 bundle.resources 가 싣는다
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/md-render");
        assert_eq!(listing(&plugin).len(), PLUGIN_FILES.len(), "{}", plugin.display());
    }
}
