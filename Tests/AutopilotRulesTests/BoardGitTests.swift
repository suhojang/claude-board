// 보드 합치기·승격 시트의 git 자리 박기·막기 규칙(Sources/AutopilotRules/BoardGit.swift) 시험(2026-10-30 ⑦ 보드 git 받침).
import Testing
import AutopilotRules
struct BoardGitRow: Row {
    let note: String
    let commonDir: String?
    let commonIno: String?
    var workGitDir: String? = nil
    var workGitIno: String? = nil
    let liveCommon: String?
    let liveCommonIno: String?
    var liveWorkIno: String? = nil
    let changed: String?
}

private let boardRows: [BoardGitRow] = [
    .init(note: "기록 없음(옛 실행)은 막지 않음", commonDir: nil, commonIno: nil, liveCommon: "/r/.git", liveCommonIno: "1:2", changed: nil),
    .init(note: "경로·inode 같음 → 통과", commonDir: "/r/.git", commonIno: "1:2", liveCommon: "/r/.git", liveCommonIno: "1:2", changed: nil),
    .init(note: "공통 git 폴더 경로가 다름 → 막음", commonDir: "/r/.git", commonIno: "1:2", liveCommon: "/evil/.git", liveCommonIno: "1:2", changed: "git 폴더가 출발 때와 달라져"),
    .init(note: "inode 가 다름(바꿔치기) → 막음", commonDir: "/r/.git", commonIno: "1:2", liveCommon: "/r/.git", liveCommonIno: "9:9", changed: "git 폴더가 출발 때와 달라져"),
    .init(note: "지금 공통 폴더를 못 읽음(nil) → 경로가 다른 것으로 막음", commonDir: "/r/.git", commonIno: "1:2", liveCommon: nil, liveCommonIno: nil, changed: "git 폴더가 출발 때와 달라져"),
    .init(note: "출발 때 inode 기록이 없으면 경로만 본다(지금 inode 를 못 읽어도 통과)", commonDir: "/r/.git", commonIno: nil, liveCommon: "/r/.git", liveCommonIno: nil, changed: nil),
    .init(note: "보안 검토 4: inode 를 기록했는데 지금 nil 이면(못 읽음) 다르다고 본다(전에는 nil 이면 통과)", commonDir: "/r/.git", commonIno: "1:2", liveCommon: "/r/.git", liveCommonIno: nil, changed: "git 폴더가 출발 때와 달라져"),
    .init(note: "보안 검토 4: 따로 작업 트리 git 폴더 inode 가 다름 → 막음", commonDir: "/r/.git", commonIno: "1:2", workGitDir: "/r/.git/worktrees/x", workGitIno: "3:4", liveCommon: "/r/.git", liveCommonIno: "1:2", liveWorkIno: "9:9", changed: "작업 트리 git 폴더가 출발 때와 달라져"),
    .init(note: "따로 작업 트리 git 폴더 inode 가 같음 → 통과", commonDir: "/r/.git", commonIno: "1:2", workGitDir: "/r/.git/worktrees/x", workGitIno: "3:4", liveCommon: "/r/.git", liveCommonIno: "1:2", liveWorkIno: "3:4", changed: nil),
]

@Suite("자율주행 보드 git 받침") struct BoardGitTests {
    @Test("git 폴더가 출발 때와 다르면 막는다", arguments: boardRows)
    func 바뀜(_ r: BoardGitRow) {
        let rec = APGitRecord(commonDir: r.commonDir, commonIno: r.commonIno, workGitDir: r.workGitDir, workGitIno: r.workGitIno)
        #expect(apBoardGitChanged(rec, liveCommon: r.liveCommon, liveCommonIno: r.liveCommonIno, liveWorkIno: r.liveWorkIno) == r.changed)
    }
    @Test("ref·객체를 읽을 때 박는 env: GIT_DIR·GIT_CEILING")
    func 박기() {
        #expect(apBoardRefsPin(APGitRecord(commonDir: nil, commonIno: nil, workGitDir: nil, workGitIno: nil), repoRoot: "/w/repo") == [])
        #expect(apBoardRefsPin(APGitRecord(commonDir: "/w/repo/.git", commonIno: "1:2", workGitDir: nil, workGitIno: nil), repoRoot: "/w/repo")
                == ["GIT_DIR=/w/repo/.git", "GIT_CEILING_DIRECTORIES=/w"])
        #expect(apBoardRefsPin(APGitRecord(commonDir: "/.git", commonIno: nil, workGitDir: nil, workGitIno: nil), repoRoot: "/repo")
                == ["GIT_DIR=/.git"], "뿌리의 부모가 / 이면 GIT_CEILING 을 넣지 않는다")
    }
}
