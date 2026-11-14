//! 중첩 git 저장소(하위 모듈) 가리기·막기 규칙. 원본: `Sources/AutopilotRules/Gitlinks.swift`.
//! git 은 작업 폴더 안의 다른 git 저장소를 하위 모듈로 보고 바깥 git 이 그 자리로 들어가 그 저장소의 filter·hook 설정을
//! 샌드박스 밖에서 돌린다. 막는 길은 원본 머리 주석과 같다: status·diff 는 `--ignore-submodules=all`,
//! switch·checkout·claude 세션 열기는 git 을 부르기 전에 색인 gitlink(mode 160000)나 `.gitmodules` 로 닫힌 쪽 확인,
//! add·stash 는 gitlink 경로를 빼는 pathspec.
//! 여기는 글(`ls-files` 출력·목록)만 다루는 순수 규칙이다. git 을 부르고 `.gitmodules` 를 보는 일은 바깥(어댑터)이 한다.
//! Windows: `git ls-files` 는 Windows 에서도 경로를 `/` 로 낸다. 글을 있는 그대로 읽으므로 `\`·`\r` 는 파일 이름의 일부로 남는다.

/// `-c core.quotePath=false ls-files -s -z`(색인만 읽어 filter 를 돌리지 않는다)의 NUL 로 나뉜 출력에서 gitlink(mode 160000) 경로.
/// 한 항목 꼴: `<mode> <object> <stage>\t<경로>`. quotePath=false·-z 라야 한글·탭·줄바꿈이 든 경로도 따옴표·escape 없이 그대로 온다.
/// 같은 경로가 여러 stage 로 나와도 한 번, 처음 나온 순서. 원본: `apGitlinkPaths(lsFilesZ:)`
///
/// 원본의 `contains` 는 유니코드 정규 동치(NFC=NFD)로 견주고 여기는 바이트로 견준다 — 같은 경로를 다른 정규형으로 두 번 내는 일은
/// git 이 한 색인에 두 항목으로 두는 경우뿐이다(그 경우 둘 다 남는다).
pub fn gitlink_paths(ls_files_z: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in ls_files_z.split('\0').filter(|e| !e.is_empty()) {
        if !entry.starts_with("160000 ") {
            continue;
        }
        let Some(tab) = entry.find('\t') else { continue };
        let path = &entry[tab + 1..];
        if !path.is_empty() && !out.iter().any(|p| p == path) {
            out.push(path.to_string());
        }
    }
    out
}

/// gitlink 경로를 add·stash 에서 빼는 pathspec(git 은 `:(exclude,literal)<경로>` 를 그 경로와 그 아래를 빼는 것으로 읽는다).
/// 없으면 빈 목록. 원본: `apGitlinkExcludes(lsFilesZ:)`
pub fn gitlink_excludes(ls_files_z: &str) -> Vec<String> {
    gitlink_paths(ls_files_z).into_iter().map(|p| format!(":(exclude,literal){p}")).collect()
}

/// 작업 폴더가 하위 모듈을 품고 있는가 — 색인에 gitlink 가 있거나(`gitlinks` 비지 않음) 작업 트리에 `.gitmodules` 가 있음.
/// 하나라도 있으면 엔진·보드가 그 자리에서 git 을 돌리지 않고 claude 를 열지 않는다(닫힌 쪽). 원본: `apHasSubmodule(gitlinks:gitmodules:)`
pub fn has_submodule(gitlinks: &[String], gitmodules: bool) -> bool {
    !gitlinks.is_empty() || gitmodules
}

/// 멈추는 까닭(엔진·보드가 글머리로 쓴다). 색인 gitlink 와 `.gitmodules` 가운데 있는 것을 적는다.
/// gitlink 는 앞 셋만 보이고 나머지는 `외 N개`. 원본: `apSubmoduleReason(gitlinks:gitmodules:)`
pub fn submodule_reason(gitlinks: &[String], gitmodules: bool) -> String {
    let mut what: Vec<String> = Vec::new();
    if !gitlinks.is_empty() {
        let shown = gitlinks.iter().take(3).map(String::as_str).collect::<Vec<_>>().join(", ");
        let more = if gitlinks.len() > 3 { format!(" 외 {}개", gitlinks.len() - 3) } else { String::new() };
        what.push(format!("색인에 중첩 git 저장소({shown}{more})"));
    }
    if gitmodules {
        what.push(".gitmodules 파일".to_string());
    }
    format!("작업 폴더에 하위 모듈({})이 있어", what.join(", "))
}
