// 중첩 git(gitlink) 가리기 규칙(Sources/AutopilotRules/Gitlinks.swift) 시험.
// `git ls-files -s` 출력에서 gitlink(mode 160000) 경로를 뽑아 add·stash 에서 빼는 pathspec 을 만든다(2026-10-30 ⑦ 보안)
import Testing
import AutopilotRules

struct GitlinkRow: Row {
    let note: String
    let lsFiles: String
    let paths: [String]
}

private let rows: [GitlinkRow] = [
    .init(note: "비었으면 없음", lsFiles: "", paths: []),
    .init(note: "보통 파일(100644)·실행 파일(100755)·링크(120000)는 gitlink 가 아님",
          lsFiles: "100644 aaaa 0\ta.txt\n100755 bbbb 0\trun.sh\n120000 cccc 0\tlink",
          paths: []),
    .init(note: "mode 160000 하나", lsFiles: "100644 aaaa 0\ta.txt\n160000 bbbb 0\tsub", paths: ["sub"]),
    .init(note: "여러 gitlink, 경로에 빈칸·슬래시", lsFiles: "160000 a 0\tvendor/lib\n160000 b 0\tsub dir\n100644 c 0\tx", paths: ["vendor/lib", "sub dir"]),
    .init(note: "같은 경로가 여러 stage 로 나와도 한 번", lsFiles: "160000 a 1\tsub\n160000 b 2\tsub\n160000 c 3\tsub", paths: ["sub"]),
    .init(note: "160000 으로 시작하는 object 이름에 속지 않음(mode 로만)", lsFiles: "100644 160000abcd 0\thash.txt", paths: []),
]

@Suite("자율주행 gitlink 가리기") struct GitlinkTests {
    @Test("색인의 gitlink 경로", arguments: rows)
    func 경로(_ r: GitlinkRow) {
        #expect(apGitlinkPaths(lsFilesStage: r.lsFiles) == r.paths)
    }
    @Test("add·stash 에서 빼는 pathspec", arguments: rows)
    func pathspec(_ r: GitlinkRow) {
        #expect(apGitlinkExcludes(lsFilesStage: r.lsFiles) == r.paths.map { ":(exclude,literal)" + $0 })
    }
}
