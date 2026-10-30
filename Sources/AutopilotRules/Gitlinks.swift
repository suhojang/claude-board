// 자율주행 규칙(AutopilotRules) — 모델이 작업 폴더에 심은 중첩 git 저장소(하위 모듈) 가리기·막기(2026-10-30 ⑦ 보안, 보안 검토 2건).
// git 은 작업 폴더 안의 다른 git 저장소를 "하위 모듈"로 보고, 바깥 git(엔진의 status·diff·switch·add·stash, claude 의 세션 열기 status,
// 판정 명령)이 그 자리로 들어가 그 저장소의 filter·hook 설정을 샌드박스 밖에서 돌린다. 모델이 그 .git/config 에 filter 드라이버를 심으면
// 임의 명령이 돈다. 막는 길:
//  ① status·diff 는 명령줄 플래그 --ignore-submodules=all 로 하위 모듈을 아예 보지 않게 한다. -c·환경의 diff.ignoreSubmodules 설정은
//     모델이 .gitmodules 에 submodule.<이름>.ignore = none 을 쓰면 진다(git 소스 set_diffopt_flags_from_submodule_config, 실측). 플래그만 이긴다.
//  ② switch·checkout·claude 세션 열기 status 는 플래그를 붙일 수 없다(switch 는 받지 않고, claude 의 git 은 엔진이 환경만 줄 수 있다).
//     그래서 git 을 부르거나 claude 세션을 열기 전에 닫힌 쪽으로 확인한다: 색인에 gitlink(mode 160000)가 있거나 작업 트리에 .gitmodules 가 있으면
//     그 자리에서 git 을 돌리지 않는다. 자율주행은 git 하위 모듈을 쓰지 않는다(단일 저장소·형제 묶음은 상대 경로).
//  ③ add·stash 는 색인의 gitlink 경로를 빼는 pathspec 으로 막는다(위 ②가 먼저 멈추므로 받침이지만, 중첩 저장소를 커밋·보관에 담지 않는 것은 맞는 동작).
import Foundation

// `-c core.quotePath=false ls-files -s -z`(색인만 읽어 filter 를 돌리지 않는다)의 NUL 로 나뉜 출력에서 gitlink(mode 160000) 경로.
// 한 항목 꼴: "<mode> <object> <stage>\t<경로>". quotePath=false·-z 라야 한글·탭·줄바꿈이 든 경로도 따옴표·escape 없이 그대로 온다(보안 검토 1).
public func apGitlinkPaths(lsFilesZ: String) -> [String] {
    var out: [String] = []
    for entry in lsFilesZ.split(separator: "\0", omittingEmptySubsequences: true) {
        guard entry.hasPrefix("160000 "), let tab = entry.firstIndex(of: "\t") else { continue }
        let path = String(entry[entry.index(after: tab)...])
        if !path.isEmpty, !out.contains(path) { out.append(path) }
    }
    return out
}

// gitlink 경로를 add·stash 에서 빼는 pathspec(git 은 ":(exclude,literal)<경로>" 를 그 경로와 그 아래를 빼는 것으로 읽는다). 없으면 빈 배열
public func apGitlinkExcludes(lsFilesZ: String) -> [String] {
    apGitlinkPaths(lsFilesZ: lsFilesZ).map { ":(exclude,literal)" + $0 }
}

// 작업 폴더가 하위 모듈을 품고 있는가 — 색인에 gitlink 가 있거나(gitlinks 비지 않음) 작업 트리에 .gitmodules 가 있음.
// 하나라도 있으면 엔진·보드가 그 자리에서 git(모델·판정·switch·커밋)을 돌리지 않고 claude 를 열지 않는다(닫힌 쪽)
public func apHasSubmodule(gitlinks: [String], gitmodules: Bool) -> Bool {
    !gitlinks.isEmpty || gitmodules
}

// 멈추는 까닭(엔진·보드가 글머리로 쓴다). 색인 gitlink 와 .gitmodules 가운데 있는 것을 적는다
public func apSubmoduleReason(gitlinks: [String], gitmodules: Bool) -> String {
    var what: [String] = []
    if !gitlinks.isEmpty {
        what.append("색인에 중첩 git 저장소(" + gitlinks.prefix(3).joined(separator: ", ") + (gitlinks.count > 3 ? " 외 \(gitlinks.count - 3)개" : "") + ")")
    }
    if gitmodules { what.append(".gitmodules 파일") }
    return "작업 폴더에 하위 모듈(" + what.joined(separator: ", ") + ")이 있어"
}
