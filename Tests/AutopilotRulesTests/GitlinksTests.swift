// 중첩 git(하위 모듈) 가리기·막기 규칙(Sources/AutopilotRules/Gitlinks.swift) 시험.
// `-c core.quotePath=false ls-files -s -z` 출력(NUL 로 나뉨)에서 gitlink(mode 160000) 경로를 뽑아 add·stash 에서 빼는 pathspec 을 만들고,
// 색인 gitlink 나 .gitmodules 가 있으면 멈춘다(2026-10-30 ⑦ 보안 검토 2건: -z 판독, .gitmodules 우회 막기)
import Testing
import AutopilotRules

struct GitlinkRow: Row {
    let note: String
    let lsFilesZ: String
    let paths: [String]
}

// \0 로 나뉜 ls-files -z 출력(항목 끝마다 NUL)
private func z(_ entries: [String]) -> String { entries.map { $0 + "\0" }.joined() }

private let rows: [GitlinkRow] = [
    .init(note: "비었으면 없음", lsFilesZ: "", paths: []),
    .init(note: "보통 파일(100644)·실행 파일(100755)·링크(120000)는 gitlink 가 아님",
          lsFilesZ: z(["100644 aaaa 0\ta.txt", "100755 bbbb 0\trun.sh", "120000 cccc 0\tlink"]), paths: []),
    .init(note: "mode 160000 하나", lsFilesZ: z(["100644 aaaa 0\ta.txt", "160000 bbbb 0\tsub"]), paths: ["sub"]),
    .init(note: "보안 검토 1: 경로에 한글·빈칸·슬래시(quotePath=false·-z 라 따옴표·escape 없이 그대로)",
          lsFilesZ: z(["160000 a 0\t모듈/하위 디렉터리", "160000 b 0\tsub dir", "100644 c 0\tx"]), paths: ["모듈/하위 디렉터리", "sub dir"]),
    .init(note: "보안 검토 1: 경로에 탭이 들어가도 첫 탭 뒤 전부가 경로", lsFilesZ: z(["160000 a 0\ta\tb"]), paths: ["a\tb"]),
    .init(note: "같은 경로가 여러 stage 로 나와도 한 번", lsFilesZ: z(["160000 a 1\tsub", "160000 b 2\tsub", "160000 c 3\tsub"]), paths: ["sub"]),
    .init(note: "160000 으로 시작하는 object 이름에 속지 않음(mode 로만)", lsFilesZ: z(["100644 160000abcd 0\thash.txt"]), paths: []),
]

@Suite("자율주행 하위 모듈 가리기") struct GitlinkTests {
    @Test("색인의 gitlink 경로(-z 판독)", arguments: rows)
    func 경로(_ r: GitlinkRow) {
        #expect(apGitlinkPaths(lsFilesZ: r.lsFilesZ) == r.paths)
    }
    @Test("add·stash 에서 빼는 pathspec", arguments: rows)
    func pathspec(_ r: GitlinkRow) {
        #expect(apGitlinkExcludes(lsFilesZ: r.lsFilesZ) == r.paths.map { ":(exclude,literal)" + $0 })
    }

    @Test("하위 모듈이 있으면 멈춘다(색인 gitlink 또는 .gitmodules)")
    func 멈춤() {
        #expect(apHasSubmodule(gitlinks: [], gitmodules: false) == false)
        #expect(apHasSubmodule(gitlinks: ["sub"], gitmodules: false) == true)
        #expect(apHasSubmodule(gitlinks: [], gitmodules: true) == true, "보안 검토 2: .gitmodules 만 있어도 멈춘다(ignore=none 이 설정을 이기므로)")
        #expect(apHasSubmodule(gitlinks: ["sub"], gitmodules: true) == true)
    }

    @Test("멈추는 까닭: 색인 gitlink 와 .gitmodules 가운데 있는 것")
    func 까닭() {
        #expect(apSubmoduleReason(gitlinks: ["sub"], gitmodules: false) == "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(sub))이 있어")
        #expect(apSubmoduleReason(gitlinks: [], gitmodules: true) == "작업 폴더에 하위 모듈(.gitmodules 파일)이 있어")
        #expect(apSubmoduleReason(gitlinks: ["a", "b"], gitmodules: true) == "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b), .gitmodules 파일)이 있어")
        #expect(apSubmoduleReason(gitlinks: ["a", "b", "c", "d"], gitmodules: false) == "작업 폴더에 하위 모듈(색인에 중첩 git 저장소(a, b, c 외 1개))이 있어")
    }
}
