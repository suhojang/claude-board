// 자율주행 규칙(AutopilotRules) — 모델이 작업 폴더 안에 심은 중첩 git 저장소(gitlink, 색인 mode 160000) 가리기(2026-10-30 ⑦ 보안).
// git 은 하위 폴더의 .git 을 "하위 모듈"로 보고, 바깥 git(엔진의 status·diff·switch·add·stash, claude 의 세션 열기 status)이 그 자리로
// 들어가 그 저장소의 filter·hook 설정을 돌린다. 모델이 그 .git/config 에 filter 드라이버를 심으면 샌드박스 밖에서 임의 명령이 돈다.
// status·diff·switch 는 diff.ignoreSubmodules=all(엔진·보드의 -c, claude 의 환경)로 하위 모듈을 아예 보지 않게 하고,
// add -A·stash 처럼 경로를 담는 명령은 색인의 gitlink 경로를 빼는 pathspec 으로 막는다(이 규칙).
// 자율주행이 쓰는 저장소는 단일 저장소이거나 형제 저장소 묶음(상대 경로)이고 git 하위 모듈을 쓰지 않으므로, 하위 모듈을 통째로 무시해도 된다.
import Foundation

// `git ls-files -s`(색인만 읽고 filter 를 돌리지 않는다)의 출력에서 gitlink(mode 160000) 경로.
// 한 줄 꼴: "<mode> <object> <stage>\t<경로>"
public func apGitlinkPaths(lsFilesStage: String) -> [String] {
    var out: [String] = []
    for line in lsFilesStage.split(separator: "\n", omittingEmptySubsequences: true) {
        guard line.hasPrefix("160000 ") else { continue }
        guard let tab = line.firstIndex(of: "\t") else { continue }
        let path = String(line[line.index(after: tab)...])
        if !path.isEmpty, !out.contains(path) { out.append(path) }
    }
    return out
}

// gitlink 경로를 add·stash 에서 빼는 pathspec(git 은 ":(exclude,literal)<경로>" 를 그 경로와 그 아래를 빼는 것으로 읽는다).
// 빼는 것이 없으면 빈 배열(그러면 부르는 쪽은 pathspec 없이 평소대로 돈다)
public func apGitlinkExcludes(lsFilesStage: String) -> [String] {
    apGitlinkPaths(lsFilesStage: lsFilesStage).map { ":(exclude,literal)" + $0 }
}
