// 자율주행 규칙(AutopilotRules) — 보드가 합치기·승격 시트에서 돌리는 git 의 자리 박기·막기(2026-10-30 ⑦ 보드 git 받침).
// 엔진이 5-4 에서 한 막기(모델이 쓸 수 있던 작업 트리의 git 폴더가 출발 때와 다르면 돌리지 않음, GIT_DIR 박기)를 보드 쪽에도 맞춰 넣는다.
// 보드가 돌리는 git 은 샌드박스 밖(보드 프로세스)이라, 모델이 실행 중·실행 뒤에 .git 을 바꿔치기했거나 중첩 git 을 심어 두면
// 사람이 시트를 열 때 그 저장소의 filter·hook 설정이 돈다. 공통 보호(safe.bareRepository·diff.ignoreSubmodules)는 보드 git 어댑터에 늘 넣고,
// 실행 기록(run.json branches.repos.<저장소>)이 있는 자리에서는 그때 적은 git 폴더로 박고(GIT_DIR), 그 폴더가 지금 다르면 돌리지 않는다.
import Foundation

// 실행 기록의 저장소 하나 git 자리(run.json branches.repos.<저장소>). 보드가 읽어 값으로 넘긴다
public struct APGitRecord: Equatable, Sendable {
    public let commonDir: String?      // 공통 git 폴더(가지·객체) — 출발 때 적은 절대 경로
    public let commonIno: String?      // 그 자리의 "장치:inode"(바꿔치기 가리기)
    public let workGitDir: String?     // 따로 작업 트리면 그 작업 트리의 git 폴더(.git/worktrees/<이름>)
    public let workGitIno: String?
    public init(commonDir: String?, commonIno: String?, workGitDir: String?, workGitIno: String?) {
        self.commonDir = commonDir; self.commonIno = commonIno; self.workGitDir = workGitDir; self.workGitIno = workGitIno
    }
}

// 저장소의 공통 git 폴더가 출발 때와 같은가(엔진 gitDirChange 와 같은 꼴). liveCommon 은 지금 rev-parse --git-common-dir 를 푼 절대 경로,
// liveCommonIno 는 그 자리의 "장치:inode". 다르면 까닭(사람에게 보이는 글머리), 같거나 기록이 없으면(옛 실행) nil.
// rev-parse 는 설정을 읽기만 하고 filter·hook 을 돌리지 않는다(보드 git 어댑터의 공통 보호 아래에서 돈다)
public func apBoardGitChanged(_ record: APGitRecord, liveCommon: String?, liveCommonIno: String?) -> String? {
    guard let c = record.commonDir else { return nil }   // 기록 없음: 막지 않고 공통 보호(safe.bareRepository)로만
    if liveCommon != c { return "git 폴더가 출발 때와 달라져" }
    if let ino = record.commonIno, liveCommonIno != nil, liveCommonIno != ino { return "git 폴더가 출발 때와 달라져" }
    return nil
}

// 보드가 ref·객체를 읽을 때(for-each-ref·rev-parse·rev-list·merge-tree·diff a...b) 박는 env. 기록의 공통 git 폴더로 GIT_DIR 을 박고,
// 저장소 뿌리 위로는 git 이 저장소를 찾지 않게 GIT_CEILING_DIRECTORIES(뿌리의 부모)를 넣는다. 기록이 없으면 빈 배열(공통 보호로만).
// GIT_WORK_TREE 는 넣지 않는다 — ref·객체만 다루고 작업 트리를 보지 않는다(작업 트리 status 는 그 자리에서 공통 보호로 돈다)
public func apBoardRefsPin(_ record: APGitRecord, repoRoot: String) -> [String] {
    guard let c = record.commonDir else { return [] }
    var env = ["GIT_DIR=" + c]
    let parent = (repoRoot as NSString).deletingLastPathComponent
    if parent != "/" && !parent.isEmpty { env.append("GIT_CEILING_DIRECTORIES=" + parent) }
    return env
}
