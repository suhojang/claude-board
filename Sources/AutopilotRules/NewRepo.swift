// 자율주행 규칙(AutopilotRules) — 주제로 시작의 "새 폴더 만들기"(설계 2장 "주제로 시작": 보드가 폴더를 만들고 git init, 빈 첫 커밋,
// 그 커밋에 승격 차례의 가지들을 만든다. 사용자 결정: 기본 dev·live·master, 출발 가지 live). 2026-10-30 2단계 5-2b 에서 새로 씀.
// 이름·자리 확인과 git 차례는 값으로 정하고, 폴더·git 은 포트(APFolders·APGitRunner)로만 부른다
import Foundation

// 새 폴더 계획 — 만들 자리, 가지(빈 첫 커밋에 모두), 처음 걸어 둘 가지, 출발 가지
public struct APNewRepoPlan: Equatable, Sendable {
    public let path: String
    public let branches: [String]
    public let head: String
    public let base: String
    public let message: String
    public init(path: String, branches: [String], head: String, base: String, message: String) {
        self.path = path; self.branches = branches; self.head = head; self.base = base; self.message = message
    }
}

public let apNewRepoBranches = ["dev", "live", "master"]
public let apNewRepoMessage = "빈 첫 커밋(Claude Board 자율주행이 새 폴더로 만듦)"

// 이름과 자리를 보고 계획을 만든다. parent 는 위치(절대 경로), name 은 새 폴더 이름, home 은 홈 폴더.
// 이름은 영문·숫자·.-_ 로(엔진이 프로젝트 이름·실행 id·따로 작업 트리 경로에 쓴다. 경로에 공백이 없게), 64자까지, 점으로 시작하지 않는다.
// 위치는 홈 폴더 아래의 있는 폴더이고, 앱 자리(~/Library)·Claude 자리(~/.claude, ~/.claude-board) 밖이다. 새 자리는 없거나 빈 폴더여야 한다
// (무엇이 든 폴더에 git init 하지 않는다 — 사람의 파일을 첫 커밋에 넣지 않게).
// 처음 걸어 둘 가지는 dev(사람이 일하는 자리, 설계 판단), 출발 가지는 live(사용자 결정)
public func apNewRepoCheck(parent: String, name: String, home: String, parentState: APPathState, targetState: APPathState) -> (plan: APNewRepoPlan?, problems: [String]) {
    var p: [String] = []
    let n = name.trimmingCharacters(in: .whitespaces)
    if n.isEmpty { p.append("새 폴더 이름을 넣어 주세요") }
    else if n.count > 64 { p.append("새 폴더 이름은 64자까지입니다") }
    else if n.range(of: "^[A-Za-z0-9][A-Za-z0-9._-]*$", options: .regularExpression) == nil {
        p.append("새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)")
    }
    var par = parent.trimmingCharacters(in: .whitespaces)
    while par.count > 1, par.hasSuffix("/") { par.removeLast() }
    let h = home.hasSuffix("/") ? String(home.dropLast()) : home
    if !par.hasPrefix("/") { p.append("위치는 / 로 시작하는 경로로 씁니다") }
    else if par != h && !par.hasPrefix(h + "/") { p.append("위치는 홈 폴더 아래여야 합니다") }
    else if ["/Library", "/.claude", "/.claude-board", "/.Trash"].contains(where: { par == h + $0 || par.hasPrefix(h + $0 + "/") }) {
        p.append("위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다")
    } else if par.split(separator: "/").contains(where: { $0 == "." || $0 == ".." }) {
        p.append("위치에 . 이나 .. 을 쓰지 않습니다")
    } else if parentState != .emptyDir && parentState != .dir {
        p.append(parentState == .missing ? "위치 폴더가 없습니다" : "위치가 폴더가 아닙니다")
    }
    let path = par + "/" + n
    if p.isEmpty {
        switch targetState {
        case .missing, .emptyDir: break
        case .dir: p.append("그 자리에 이미 무엇이 든 폴더가 있습니다. 기존 저장소로 고르거나 다른 이름을 씁니다")
        case .other: p.append("그 자리에 폴더가 아닌 것이 있습니다")
        }
    }
    guard p.isEmpty else { return (nil, p) }
    return (APNewRepoPlan(path: path, branches: apNewRepoBranches, head: "dev", base: "live", message: apNewRepoMessage), [])
}

// git 차례 — 처음 가지 이름으로 init, 빈 첫 커밋(사람 이름은 사용자 git 설정, 없으면 보드 이름), 나머지 가지를 그 커밋에.
// 사용자 전역 설정의 hooks·서명 설정은 쓰지 않는다(어댑터의 보호와 함께, 서명은 -c 로 끈다)
public func apNewRepoSteps(_ plan: APNewRepoPlan, identity: (name: String, email: String)?) -> [[String]] {
    let who = identity ?? ("Claude Board", "claude-board@localhost")
    var s: [[String]] = [["init", "-q", "-b", plan.head]]
    s.append(["-c", "user.name=\(who.name)", "-c", "user.email=\(who.email)", "-c", "commit.gpgsign=false",
              "commit", "-q", "--allow-empty", "--no-verify", "-m", plan.message])
    for b in plan.branches where b != plan.head { s.append(["branch", b]) }
    return s
}

public struct APCreateResult: Equatable, Sendable {
    public var ok = false
    public var made = false            // 폴더를 새로 만들었나(빈 폴더였으면 false)
    public var commit: String?         // 빈 첫 커밋
    public var done: [String] = []     // 한 일(쉬운 말)
    public var error: String?
    public init() {}
}

// 유스케이스: 새 폴더 만들기 — 계획대로 폴더(없을 때)·git init·빈 첫 커밋·가지. 하다 실패하면 거기서 멈추고 까닭을 돌려준다
// (만든 것을 지우지 않는다 — 사람이 보고 정리한다). 끝에 가지 셋이 모두 빈 첫 커밋을 가리키는지 본다
public func apCreateRepo(_ plan: APNewRepoPlan, folders: APFolders, git: APGitRunner) -> APCreateResult {
    var r = APCreateResult()
    switch folders.state(plan.path) {
    case .missing:
        guard folders.makeDir(plan.path) else { r.error = "폴더를 만들지 못했습니다: \(plan.path)"; return r }
        r.made = true
        r.done.append("폴더를 만들었습니다")
    case .emptyDir: r.done.append("빈 폴더를 그대로 씁니다")
    case .dir, .other: r.error = "그 자리에 이미 무엇이 있어 만들지 않았습니다: \(plan.path)"; return r
    }
    for args in apNewRepoSteps(plan, identity: git.identity()) {
        let x = git.run(args, in: plan.path)
        guard x.code == 0 else { r.error = "git \(args.first { !$0.hasPrefix("-") && !$0.contains("=") } ?? "") 이 실패했습니다(종료 코드 \(x.code))" + (x.out.isEmpty ? "" : ": " + x.out); return r }
    }
    r.done.append("git init(처음 가지 \(plan.head))")
    var heads = Set<String>()
    for b in plan.branches {
        let x = git.run(["rev-parse", "--verify", "-q", "refs/heads/" + b], in: plan.path)
        guard x.code == 0, !x.out.isEmpty else { r.error = "가지 \(b) 를 만들지 못했습니다"; return r }
        heads.insert(x.out)
    }
    guard heads.count == 1, let c = heads.first else { r.error = "가지가 같은 첫 커밋을 가리키지 않습니다"; return r }
    r.commit = c
    r.done.append("빈 첫 커밋 \(c.prefix(7)) 에 가지 " + plan.branches.joined(separator: "·"))
    r.ok = true
    return r
}
