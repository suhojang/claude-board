// 자율주행 규칙(AutopilotRules) — 판정 샌드박스(5단계 ⑪-1, 2026-10-30, 설계 4장 "격리" 의 남는 한계 "판정 명령은 엔진이 샌드박스 밖에서 돌린다" 를 없앰).
// 판정(판정 명령·시나리오)은 모델이 쓴 코드를 부를 수 있다(작업 폴더의 시험·앱). 그래서 엔진이 macOS Seatbelt(`sandbox-exec -f <프로필>`)로 돌린다.
// 원칙: 판정은 모델 세션보다 넓게 쓰거나 읽지 않는다(이번 증거 폴더와 판정 임시 폴더만 더). 여기는 그 프로필 글을 만드는 순수 함수다.
// - 쓰기: 처음에 모두 막고 → 작업 폴더·write_roots·판정 임시 폴더·작업 트리의 git 폴더를 열고 → 자율주행 뿌리·~/.claude·키체인·LaunchAgents·하네스 자리·
//   다른 가지·시나리오 원본을 다시 막고 → 이번 증거 폴더를 마지막에 연다(Seatbelt 는 뒤 규칙이 이긴다, 2026-10-30 sandbox-exec 로 잼)
// - 읽기: 키체인·엔진 전용 뿌리·모든 실행의 engine 폴더·read_deny·올릴 글은 막는다(모델과 같은 목록)
// - 네트워크: 127.0.0.1(localhost)만 — 나가기·묶기·받기. 이름 풀이(mDNSResponder)는 막혀도 localhost 는 풀린다(잼). unix socket 은 판정 임시 폴더 안과 사람이 정한 것만
// 경로는 진짜 경로(/tmp → /private/tmp, /var → /private/var)로 넘겨야 한다(Seatbelt 는 풀린 경로로 맞춘다). 그 일은 어댑터(엔진)가 한다
import Foundation

public struct APJudgeSandbox: Equatable, Sendable {
    public var write: [String]        // 쓰기 허용 폴더
    public var denyWrite: [String]    // 쓰기 금지(폴더·파일, 조각에 * 글롭)
    public var evidence: String       // 이번 증거 폴더(쓰기 금지보다 뒤라 이긴다)
    public var denyRead: [String]     // 읽기 금지(조각에 * 글롭)
    public var socketDirs: [String]   // unix socket 을 만들고 닿을 수 있는 폴더(판정 임시 폴더·증거 폴더)
    public var unixSockets: [String]  // 그 밖에 닿을 수 있는 socket 파일(실행 check_unix_sockets, 사람이 정함)
    public init(write: [String], denyWrite: [String], evidence: String, denyRead: [String], socketDirs: [String], unixSockets: [String] = []) {
        self.write = write; self.denyWrite = denyWrite; self.evidence = evidence; self.denyRead = denyRead
        self.socketDirs = socketDirs; self.unixSockets = unixSockets
    }
}

// 판정이 늘 쓸 수 있는 장치 파일(출력 버리기·터미널·난수·dtrace 도우미). /dev/fd 와 터미널은 꼴로
private let devLiterals = ["/dev/null", "/dev/zero", "/dev/tty", "/dev/dtracehelper", "/dev/random", "/dev/urandom", "/dev/stdout", "/dev/stderr"]

private func sbString(_ s: String) -> String { "\"" + s + "\"" }

// 정규식 글자를 그대로 쓰게 이스케이프(* 는 경로 조각 하나로 바꿈)
private func sbRegexBody(_ p: String) -> String {
    var out = ""
    for ch in p {
        switch ch {
        case "*": out += "[^/]+"
        case ".", "+", "?", "(", ")", "[", "]", "{", "}", "^", "$", "|", "\\": out += "\\" + String(ch)
        default: out.append(ch)
        }
    }
    return out
}

// 경로 하나를 Seatbelt 거르개로: 글롭이 없으면 (subpath "…"), 있으면 (regex #"^…(/|$)"). 절대 경로가 아니거나 따옴표·조절 글자가 있으면 nil
public func apSBPLFilter(_ path: String) -> String? {
    guard path.hasPrefix("/"), !path.unicodeScalars.contains(where: { $0.value < 0x20 || $0 == "\"" || $0.value == 0x7f }) else { return nil }
    var p = path
    while p.count > 1, p.hasSuffix("/") { p.removeLast() }
    if p.contains("*") { return "(regex #\"^" + sbRegexBody(p) + "(/|$)\")" }
    return "(subpath " + sbString(p.replacingOccurrences(of: "\\", with: "\\\\")) + ")"
}

// 프로필 글. 나타낼 수 없는 경로는 빼고 skipped 로 돌려준다(엔진이 진행 기록에 남김)
public func apJudgeProfile(_ s: APJudgeSandbox) -> (text: String, skipped: [String]) {
    var skipped: [String] = []
    func filters(_ ps: [String]) -> [String] {
        var seen = Set<String>(), out: [String] = []
        for p in ps {
            guard let f = apSBPLFilter(p) else { skipped.append(p); continue }
            if seen.insert(f).inserted { out.append(f) }
        }
        return out
    }
    func rule(_ head: String, _ fs: [String]) -> String? { fs.isEmpty ? nil : "(" + head + "\n  " + fs.joined(separator: "\n  ") + ")" }
    var lines = [
        ";; Claude Board 자율주행 판정 샌드박스(엔진이 만든다, 고치지 않는다)",
        "(version 1)",
        "(allow default)",
        ";; 쓰기: 모두 막고 정한 곳만 연다",
        "(deny file-write* (subpath \"/\"))",
        "(allow file-write*\n  " + devLiterals.map { "(literal " + sbString($0) + ")" }.joined(separator: "\n  ")
            + "\n  (regex #\"^/dev/fd/[0-9]+$\")\n  (regex #\"^/dev/ttys[0-9]+$\"))",
    ]
    if let r = rule("allow file-write*", filters(s.write)) { lines.append(r) }
    if let r = rule("deny file-write*", filters(s.denyWrite)) { lines.append(";; 쓰기 범위 안에서도 막는 곳(자율주행 뿌리·하네스 자리·다른 가지·시나리오 원본 등)"); lines.append(r) }
    if let r = rule("allow file-write*", filters([s.evidence])) { lines.append(";; 이번 증거 폴더(뒤 규칙이 이긴다)"); lines.append(r) }
    if let r = rule("deny file-read*", filters(s.denyRead)) { lines.append(";; 읽기 금지(키체인·엔진 전용 자리·올릴 글 등)"); lines.append(r) }
    lines.append(";; 네트워크: 127.0.0.1(localhost)만")
    lines.append("(deny network*)")
    lines.append("(allow network-outbound (remote ip \"localhost:*\"))")
    lines.append("(allow network-bind network-inbound (local ip \"localhost:*\"))")
    let socks = filters(s.socketDirs)
    if !socks.isEmpty {
        lines.append(";; unix socket: 판정 임시 폴더·증거 폴더 안만(브라우저의 SingletonSocket 등)")
        lines.append("(allow network-bind network-outbound network-inbound\n  " + socks.map { "(local unix-socket " + $0 + ")" }.joined(separator: "\n  ")
                     + "\n  " + socks.map { "(remote unix-socket " + $0 + ")" }.joined(separator: "\n  ") + ")")
    }
    let exact = s.unixSockets.filter { $0.hasPrefix("/") && !$0.contains("\"") }
    skipped += s.unixSockets.filter { !exact.contains($0) }
    if !exact.isEmpty {
        lines.append(";; 사람이 정한 unix socket(check_unix_sockets)")
        lines.append("(allow network-outbound\n  " + exact.map { "(remote unix-socket (path-literal " + sbString($0) + "))" }.joined(separator: "\n  ") + ")")
    }
    return (lines.joined(separator: "\n") + "\n", skipped)
}

// 출발 전 status 와 증거 요약에 쓰는 한 줄
public func apJudgeSandboxNote(on: Bool) -> String {
    on ? "판정은 샌드박스 안에서 돕니다(쓰기: 작업 폴더·쓰기 범위·이번 증거 폴더·판정 임시 폴더, 네트워크: 127.0.0.1 만, 키체인·자율주행 뿌리·~/.claude 막음)"
       : "판정은 샌드박스 밖에서 돕니다(check_sandbox: false, 사람이 정함). 판정이 부르는 모델이 쓴 코드도 밖에서 돕니다"
}
