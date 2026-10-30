// 자율주행 규칙의 포트 — 규칙이 바깥(파일·git·HTTP·시각)을 부르는 자리(2026-10-30 2단계 5-2b 에서 처음 생김).
// 새 폴더 만들기(NewRepo.swift)가 바깥(폴더·git)을, 기술 스택 감지(Stack.swift)가 바깥(저장소 빌드 파일)을,
// 버전 목록(Versions.swift)이 바깥(공식 목록 받기·하루 캐시)을, 원문 지우기(Purge.swift)가 바깥(실행 폴더·지운 기록)을,
// 보고서 그리기(Report.swift, 3단계)가 바깥(실행 폴더·기록 저장소 읽기, 보고서 파일 쓰기)을 부르는 자리.
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

// MARK: - 보고서 그리기 (Report.swift)

// 보고서를 그릴 기록 — 실행 폴더(엔진: 메모리의 run·queue 와 단위 파일, 보드: 실행 폴더 파일)나 기록 저장소(보드: 원문을 지운 실행).
// 모두 run.json·queue.json·engine.json 과 같은 꼴의 사전으로 내놓는다(기록 저장소 어댑터가 행을 그 꼴로 바꾼다)
public enum APReportOrigin: String, Equatable, Sendable { case folder, records }   // 기록의 바탕: 실행 폴더 · 기록 저장소

public protocol APReportSource {
    var origin: APReportOrigin { get }
    func run() -> [String: Any]?
    func queue() -> [[String: Any]]
    func engine(_ n: Int) -> [String: Any]?
    /// 모델이 쓴 결과 기록 글(보통 파일이고 크기 상한 아래일 때만, 어댑터가 본다). 없으면 nil
    func result(_ n: Int) -> String?
}

public enum APReportFile: Equatable, Hashable, Sendable {
    case run           // runs/<실행 id>/report.html — 실행 하나(단위 모두)
    case unit(Int)     // runs/<실행 id>/units/<n>/report.html — 단위 하나
}

// 보고서 쓸 곳 — 되면 nil, 아니면 까닭. 링크를 따라가지 않게 쓰는 것은 어댑터 몫(엔진: 엔진 전용 폴더에 쓰고 rename)
public protocol APReportSink {
    func write(_ file: APReportFile, _ html: String) -> String?
}

// MARK: - 바깥 연결과 올릴 글 (Connections.swift·Outbox.swift, 4단계 ⑩-1)

// HTTP 요청 하나 — 어댑터는 리다이렉트를 따르지 않고(3xx 는 그대로 돌려줌), 쿠키·캐시를 쓰지 않고, 응답 본문에 크기 상한을 둔다.
// auth 는 보내기 직전에 만든 인증 머리(이름·값)이고 어댑터는 이것을 남기지 않는다(기록·파일·환경 변수 어디에도)
public protocol APHTTP {
    func send(_ request: APHTTPRequest, auth: (name: String, value: String)?) -> APHTTPResponse
}

// 키체인의 연결 비밀(API 토큰·webhook 주소) — 항목 이름은 apSecretService(dev:), 계정 이름은 연결 id. 읽기만(쓰기·지우기는 보드 설정 화면)
public protocol APSecrets {
    func read(_ id: String) -> String?
    func exists(_ id: String) -> Bool
}

// 429 뒤 기다리기(가짜는 기다린 초만 적는다)
public protocol APWaiter {
    func wait(_ seconds: Double)
}

// 실행 폴더의 올릴 글(outbox/<글 id>.json) — 글마다 파일 하나, 쓰기는 임시 파일 + rename, 올리기는 글마다 잠금(flock) 안에서.
// 잠금을 잡은 쪽이 올리는 동안 다른 쪽(엔진·보드가 부른 엔진 명령)은 기다리거나 비켜 간다. 프로세스가 죽으면 잠금은 저절로 풀린다
public protocol APOutboxStore {
    func ids() -> [String]
    func read(_ id: String) -> APOutboxItem?
    @discardableResult func write(_ item: APOutboxItem) -> Bool
    /// 같은 id 의 글이 없을 때만 만든다(있으면 false — 다시 떠서 같은 글을 또 만들지 않게)
    func create(_ item: APOutboxItem) -> Bool
    /// 그 글의 잠금을 잡고 body 를 돌린다. 정한 시간 안에 못 잡으면 nil(다른 곳에서 올리는 중)
    func withLock<T>(_ id: String, _ body: () -> T) -> T?
    /// 첨부할 실행 폴더 파일(report.html) — 링크가 아닌 보통 파일이고 상한 아래일 때만
    func attachment(_ name: String) -> Data?
}

// MARK: - 바깥 읽기와 근거 묶음 (Reading.swift·Evidence.swift, 4단계 ⑩-2)
// 읽기 요청은 위 APHTTP·APSecrets·APWaiter 를 그대로 쓴다(리다이렉트 안 따라감·응답 크기 상한·인증 머리는 보내기 직전에만)

// 근거 묶음 폴더 하나(<묶음>/MANIFEST.json 과 메타·그림 파일) — 이름은 규칙이 만든 꼴만(폴더 밖을 가리키지 않음).
// 쓰기는 임시 파일 + rename 이고 파일은 0444(세션에는 읽기 전용), 링크를 따라가지 않는다. 해시는 어댑터가 쓴 글자로 낸다
public protocol APEvidenceStore {
    func read(_ name: String) -> Data?
    /// 쓴 글자의 sha256(16진수). 못 쓰면 nil
    func write(_ name: String, _ data: Data) -> String?
    func remove(_ name: String)
}
