// 자율주행 규칙의 포트 — 규칙이 바깥(파일·git·HTTP·시각)을 부르는 자리(2026-10-30 2단계 5-2b 에서 처음 생김).
// 새 폴더 만들기(NewRepo.swift)가 바깥(폴더·git)을, 기술 스택 감지(Stack.swift)가 바깥(저장소 빌드 파일)을,
// 버전 목록(Versions.swift)이 바깥(공식 목록 받기·하루 캐시)을, 원문 지우기(Purge.swift)가 바깥(실행 폴더·지운 기록)을 부르는 자리.
// 진짜 어댑터는 보드(Sources/ClaudeBoard/AutopilotTopic.swift)에, 가짜 어댑터는 Tests/AutopilotRulesTests 에만 둔다(BoardRules 의 Ports.swift 와 같은 꼴, 이슈 #9).
// 이름의 AP 는 보드 자율주행 절의 이름 짓기를 따르고, 보드가 BoardRules 도 import 하므로 겹치지 않게 한다(BoardRules.Clock 과 APClock).
// 포트는 역할로 나눈다(범용 파일 포트를 두면 시험이 경로 글자 시험이 된다)
import Foundation

// 시계 — 하루 캐시·지운 기록의 시각
public protocol APClock: Sendable {
    var now: Date { get }
}

// MARK: - 새 폴더 만들기 (NewRepo.swift)

// 경로가 어떤 상태인가 — 새 폴더 자리와 그 위치(부모)를 볼 때
public enum APPathState: Equatable, Sendable {
    case missing      // 없음
    case emptyDir     // 빈 폴더(숨은 파일 .DS_Store 만 있어도 빈 것으로 본다)
    case dir          // 무엇이 든 폴더
    case other        // 파일·링크 등 폴더가 아님
}

public protocol APFolders: Sendable {
    func state(_ path: String) -> APPathState
    /// 폴더 하나를 만든다(부모는 만들지 않는다). 되면 true
    func makeDir(_ path: String) -> Bool
}

// git — 보드가 부르는 git 과 같은 보호(저장소·전역 설정의 hooks·fsmonitor 를 쓰지 않음)로 돈다. 표준 출력만 받는다
public protocol APGitRunner: Sendable {
    func run(_ args: [String], in dir: String) -> (code: Int32, out: String)
    /// 커밋에 적을 사람 — 사용자 전역 git 설정의 user.name·user.email(설정을 읽기만 한다). 없으면 nil
    func identity() -> (name: String, email: String)?
}

// MARK: - 기술 스택 감지 (Stack.swift)

// 저장소의 빌드·설정 파일 — 규칙이 고른 상대 경로만 읽는다(앞부분만, 크기 상한은 어댑터가). 없거나 글이 아니면 nil
public protocol APStackFiles: Sendable {
    func read(_ root: String, _ relative: String) -> String?
}

// MARK: - 버전 목록 (Versions.swift)

// 공식 목록 받기 — 고정한 공개 주소(APFeed)만 인증 없이 GET 한다. 사용자 자료는 보내지 않는다. 200 일 때만 본문
public protocol APVersionFeeds: Sendable {
    func fetch(_ feed: APFeed) -> Data?
}

// 받은 목록의 하루 캐시(보드 자율주행 뿌리 아래 파일) — 내용과 쓴 시각
public protocol APVersionCache: Sendable {
    func read() -> (data: Data, at: Date)?
    func write(_ data: Data)
}

// MARK: - 원문 지우기 (Purge.swift)

// 끝난 실행의 실행 폴더(runs/<실행 id>/)와 지운 기록(purged/<실행 id>.json)
public protocol APRawFiles: Sendable {
    /// 그 자리가 링크가 아닌 진짜 폴더인가
    func isRealDir(_ path: String) -> Bool
    /// 안의 파일 수와 바이트(링크는 따라가지 않는다)
    func usage(_ path: String) -> (files: Int, bytes: Int64)
    /// 폴더를 통째로 지운다. 되면 nil, 아니면 까닭
    func remove(_ path: String) -> String?
    /// 지운 기록을 쓴다(실행 id 마다 한 파일). 되면 true
    func writeRecord(_ run: String, _ data: Data) -> Bool
}
