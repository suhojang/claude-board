// 업데이트 판단(Updates.swift) 시험 — 최신 릴리스 VERSION 받기를 가짜 어댑터로 넣어 네트워크 없이 돌린다.
// 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다). 가짜는 받은 횟수를 세어 개발용 빌드가 받지 않는지까지 본다.
// 기대값은 Updater.check 에 있던 지금 동작 그대로다(버그 의심도 "지금 동작" 으로 적고 고치지 않는다).
// 다만 자릿수가 다른 버전 견주기는 고친 동작이다(이슈 #9 3차): 모자란 자리를 0 으로 채워 견주므로 3.2 와 3.2.0 은 같다
import Foundation
import Testing
import BoardRules

// MARK: - 가짜 어댑터 (시험마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

private final class FakeReleases: ReleaseVersions, @unchecked Sendable {
    var status: Int?
    var body: Data?
    private(set) var fetches = 0
    init(_ status: Int?, _ body: String?) { self.status = status; self.body = body.map { Data($0.utf8) } }
    func fetchLatest(_ done: @escaping (_ status: Int?, _ body: Data?) -> Void) {
        fetches += 1
        done(status, body)   // 그 자리에서 부른다 — 진짜는 URLSession 의 큐
    }
}

// MARK: - VERSION 본문 검증

struct VersionRow: Row {
    let note: String
    let status: Int?
    let body: Data?
    let expected: String?
    init(note: String, status: Int?, body: String?, expected: String?) {
        self.note = note; self.status = status; self.body = body.map { Data($0.utf8) }; self.expected = expected
    }
    init(note: String, status: Int?, bytes: [UInt8], expected: String?) {
        self.note = note; self.status = status; self.body = Data(bytes); self.expected = expected
    }
}

private let versionRows: [VersionRow] = [
    .init(note: "버전 그대로", status: 200, body: "3.2.1", expected: "3.2.1"),
    .init(note: "줄바꿈이 붙은 본문", status: 200, body: "3.2.1\n", expected: "3.2.1"),
    .init(note: "앞뒤 공백·CRLF", status: 200, body: "  3.2.1 \r\n", expected: "3.2.1"),
    .init(note: "한 자리", status: 200, body: "3", expected: "3"),
    .init(note: "네 자리", status: 200, body: "3.2.1.4", expected: "3.2.1.4"),
    .init(note: "v 가 붙으면 아니다", status: 200, body: "v3.2.1", expected: nil),
    .init(note: "빈 본문", status: 200, body: "", expected: nil),
    .init(note: "공백만", status: 200, body: "  \n\t", expected: nil),
    .init(note: "점으로 끝나면 아니다", status: 200, body: "3.", expected: nil),
    .init(note: "점으로 시작하면 아니다", status: 200, body: ".3", expected: nil),
    .init(note: "꼬리표가 붙으면 아니다", status: 200, body: "3.2.1-beta", expected: nil),
    .init(note: "두 줄이면 아니다", status: 200, body: "3.2.1\n3.2.2", expected: nil),
    .init(note: "가운데 공백이면 아니다", status: 200, body: "3.2 .1", expected: nil),
    .init(note: "HTML(오류 쪽)이면 아니다", status: 200, body: "<html>Not Found</html>", expected: nil),
    .init(note: "200 이 아니면 아니다(본문이 버전이어도)", status: 404, body: "3.2.1", expected: nil),
    .init(note: "201 도 아니다", status: 201, body: "3.2.1", expected: nil),
    .init(note: "응답 없음(연결 실패)", status: nil, body: nil, expected: nil),
    .init(note: "상태 없이 본문만", status: nil, body: "3.2.1", expected: nil),
    .init(note: "200 인데 본문 없음", status: 200, body: nil, expected: nil),
    .init(note: "UTF-8 이 아님", status: 200, bytes: [0x33, 0x2E, 0xFF, 0xFE], expected: nil),
]

@Suite("업데이트: VERSION 본문") struct UpdateVersionTests {
    @Test("200·UTF-8·앞뒤 공백 자르기·숫자(.숫자)… 꼴일 때만 버전", arguments: versionRows)
    func 본문(_ r: VersionRow) {
        #expect(releaseVersion(status: r.status, body: r.body) == r.expected)
    }
}

// MARK: - 새 버전인지

@Suite("업데이트: 새 버전인지") struct UpdateCompareTests {
    @Test("자리마다 숫자로 견준다", arguments: [
        ("3.10.0", "3.9.0", true),       // 글자로 견주면 거꾸로 나온다
        ("3.2.10", "3.2.9", true),
        ("4.0.0", "3.99.99", true),
        ("3.2.1", "3.2.1", false),       // 같으면 새 버전이 아니다
        ("3.2.0", "3.2.1", false),       // 더 낮으면 아니다
        ("3.9.0", "3.10.0", false),
        ("03.2", "3.2", false),          // 앞의 0 은 같은 수
        ("3.02", "3.2", false),
        ("3.2.0", "3.2", false),         // 자릿수가 다르면 모자란 자리를 0 으로 채운다 — 같은 버전
        ("3.2", "3.2.0", false),
        ("3.0.0", "3", false),
        ("3", "3.0.0", false),
        ("3.2.1", "3.2", true),          // 채운 0 보다 크면 새 버전
        ("3.2", "3.2.1", false),
        ("3.0.1", "3", true),
        ("3.2.0.1", "3.2", true),
        ("3.10.0", "3.9.9", true),       // 자리마다 숫자로
        ("3.9.9", "3.10", false),
        ("4", "3.99.99", true),
        ("3", "2.9.9", true),
        ("99999999999999999999.0", "3.2", true),   // Int 를 넘는 자리도 숫자로 견준다
    ])
    func 견주기(_ v: String, _ current: String, _ expected: Bool) {
        #expect(isNewerVersion(v, than: current) == expected)
    }

    @Test("확인하는 빌드 — 빈 버전과 개발용 빌드(1.0)만 빼고", arguments: [
        ("", false), ("1.0", false), ("3.2.1", true), ("1.0.0", true), ("1", true), ("2.12", true),
    ])
    func 확인하는빌드(_ current: String, _ expected: Bool) {
        #expect(updateEligible(current) == expected)
    }
}

// MARK: - 확인 결과

struct ResultRow: Row {
    let note: String
    let status: Int?
    let body: String?
    let current: String
    let expected: UpdateCheckResult
}

private let resultRows: [ResultRow] = [
    .init(note: "새 버전", status: 200, body: "3.3.0\n", current: "3.2.1", expected: .newer("3.3.0")),
    .init(note: "3.10.0 은 3.9.0 보다 새 버전", status: 200, body: "3.10.0", current: "3.9.0", expected: .newer("3.10.0")),
    .init(note: "자릿수만 다른 같은 버전(3.2.0 과 3.2)은 최신", status: 200, body: "3.2.0", current: "3.2", expected: .upToDate),
    .init(note: "3.2.1 은 3.2 보다 새 버전", status: 200, body: "3.2.1", current: "3.2", expected: .newer("3.2.1")),
    .init(note: "같은 버전이면 최신 — 알던 새 버전을 지운다", status: 200, body: "3.2.1", current: "3.2.1", expected: .upToDate),
    .init(note: "더 낮은 버전도 최신", status: 200, body: "3.2.0", current: "3.2.1", expected: .upToDate),
    .init(note: "본문이 버전이 아니면 실패 — 알던 새 버전을 그대로 둔다", status: 200, body: "v3.3.0", current: "3.2.1", expected: .failed),
    .init(note: "200 이 아니면 실패", status: 500, body: "3.3.0", current: "3.2.1", expected: .failed),
    .init(note: "응답 없음은 실패", status: nil, body: nil, current: "3.2.1", expected: .failed),
]

@Suite("업데이트: 확인 결과") struct UpdateResultTests {
    @Test("받은 응답 → 실패·최신·새 버전", arguments: resultRows)
    func 결과(_ r: ResultRow) {
        #expect(updateCheckResult(status: r.status, body: r.body.map { Data($0.utf8) }, current: r.current) == r.expected)
    }

    @Test("손으로 확인했을 때 알림 줄 안내 넷 — 새 버전이면 새 버전 알림 줄이 대신 떠서 없다")
    func 안내() {
        let dev = UpdateCheckResult.devBuild.notice(current: "1.0")
        #expect(dev?.icon == "hammer.fill")
        #expect(dev?.text == "개발용 빌드(1.0)는 업데이트를 확인하지 않습니다")
        let failed = UpdateCheckResult.failed.notice(current: "3.2.1")
        #expect(failed?.icon == "exclamationmark.triangle.fill")
        #expect(failed?.text == "업데이트를 확인하지 못했습니다")
        let latest = UpdateCheckResult.upToDate.notice(current: "3.2.1")
        #expect(latest?.icon == "checkmark.circle.fill")
        #expect(latest?.text == "최신 버전입니다 (3.2.1)")
        #expect(UpdateCheckResult.newer("3.3.0").notice(current: "3.2.1") == nil)
    }
}

// MARK: - 유스케이스: 업데이트 확인 한 번

@Suite("업데이트: 확인 한 번") struct UpdateCheckTests {
    @Test("확인하지 않는 빌드(1.0·빈 버전)는 받지 않고 바로 개발용 빌드", arguments: ["1.0", ""])
    func 개발빌드(_ current: String) {
        let releases = FakeReleases(200, "9.9.9")
        var results: [UpdateCheckResult] = []
        checkForUpdate(current: current, releases: releases) { results.append($0) }
        #expect(results == [.devBuild])
        #expect(releases.fetches == 0)
    }

    @Test("배포판은 한 번 받아 가른 결과를 한 번 넘긴다", arguments: [
        (200, "3.3.0", UpdateCheckResult.newer("3.3.0")),
        (200, "3.2.1", .upToDate),
        (200, "", .failed),
        (404, "3.3.0", .failed),
    ])
    func 배포판(_ status: Int, _ body: String, _ expected: UpdateCheckResult) {
        let releases = FakeReleases(status, body)
        var results: [UpdateCheckResult] = []
        checkForUpdate(current: "3.2.1", releases: releases) { results.append($0) }
        #expect(results == [expected])
        #expect(releases.fetches == 1)
    }

    @Test("응답이 없으면 실패")
    func 응답없음() {
        let releases = FakeReleases(nil, nil)
        var results: [UpdateCheckResult] = []
        checkForUpdate(current: "3.2.1", releases: releases) { results.append($0) }
        #expect(results == [.failed])
        #expect(releases.fetches == 1)
    }
}
