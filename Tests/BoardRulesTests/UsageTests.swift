// 사용량 규칙 시험 — 사용량 캐시·로그인 정보·사용량 API·시계를 가짜 어댑터로 넣어 파일·키체인·네트워크 없이 돌린다.
// 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다). 가짜는 부른 것을 적어 두어 캐시를 언제 읽고 쓰는지·API 를 언제 부르는지까지 확인한다.
// 기대값은 UsageStore 에 있던 지금 동작 그대로다. 다만 캐시 신선도와 빈 한도 응답은 고친 동작이다(이슈 #9 2차): 못 푸는 캐시와
// 수정 시각이 미래인 캐시는 받고, 푼 한도가 하나도 없는 응답은 실패다. 모델별 주간의 id 겹침과 limits 에 섞인 객체 아닌 원소도
// 고친 동작이다(이슈 #9 3차): 겹치는 id 는 뒤의 것에 -2·-3 … 을 붙이고, 객체 아닌 원소만 뺀다.
// 시험의 토큰은 가짜 글자이고 note·#expect 메시지에 토큰을 넣지 않는다
import Foundation
import Testing
import BoardRules

private let u0 = Date(timeIntervalSince1970: 1_000_000)   // 시험의 지금
private let fakeToken = "fake-token-for-tests"              // 진짜 토큰이 아니다

// MARK: - 가짜 어댑터 (시험마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

private final class FakeUsageCache: UsageCache, @unchecked Sendable {
    var modified: Date?                 // 캐시 파일 수정 시각, nil 이면 파일 없음
    var content: Data?                  // 내용, nil 이면 못 읽음
    var writeAt: Date?                  // 쓰면 수정 시각이 이것이 된다(흐름 시험)
    private(set) var calls: [String] = []      // "modified"·"read"·"write"·"remove" 부른 차례
    private(set) var written: [Data] = []
    func modifiedAt() -> Date? { calls.append("modified"); return modified }
    func read() -> Data? { calls.append("read"); return content }
    func write(_ data: Data) {
        calls.append("write"); written.append(data)
        content = data
        if let writeAt { modified = writeAt }
    }
    func remove() { calls.append("remove"); modified = nil; content = nil }
}

private final class FakeCredentials: LoginCredentials, @unchecked Sendable {
    var raw: Data?
    private(set) var reads = 0
    func read() -> Data? { reads += 1; return raw }
}

private final class FakeUsageAPI: UsageAPI, @unchecked Sendable {
    var reply: Data?                            // 200 일 때의 본문, nil 이면 200 아님(연결 실패·시간 초과 포함)
    private(set) var tokens: [String] = []      // 받은 토큰(가짜 글자) — 부른 횟수도 이것으로 센다
    func fetch(token: String) -> Data? { tokens.append(token); return reply }
}

private struct FixedClock: Clock { let now: Date }

private func data(_ s: String?) -> Data? { s.map { Data($0.utf8) } }

// 한도를 견주기 쉬운 값으로 — 다시 놓는 시각은 1970 기준 밀리초(형식기가 소수 초를 밀리초까지 읽는다)
private struct L: Equatable, Sendable, CustomStringConvertible {
    let id: String, label: String, pct: Double, resets: Int64?
    init(_ id: String, _ label: String, _ pct: Double, _ resets: Int64? = nil) { self.id = id; self.label = label; self.pct = pct; self.resets = resets }
    init(_ l: Limit) { self.init(l.id, l.label, l.pct, l.resets.map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) }) }
    var description: String { "\(id)|\(label)|\(pct)|\(resets.map(String.init) ?? "nil")" }
}

// 응답 예 — 실제 응답(2026-10)에서 쓰는 필드만 남긴 것. resets_at 은 마이크로초까지 온다
private let msFive: Int64 = 1_793_184_600_697           // 2026-10-28T10:50:00.697299+00:00
private let msWeek: Int64 = 1_793_257_200_697           // 2026-10-29T07:00:00.697320+00:00
private let msFable: Int64 = 1_793_257_199_697          // 2026-10-29T06:59:59.697480+00:00
private let realistic = #"""
{"five_hour":{"utilization":5.0,"resets_at":"2026-10-28T10:50:00.697299+00:00","limit_dollars":null},
 "seven_day":{"utilization":13.0,"resets_at":"2026-10-29T07:00:00.697320+00:00"},
 "seven_day_opus":null,
 "limits":[
  {"kind":"session","percent":5,"resets_at":"2026-10-28T10:50:00.697299+00:00","scope":null},
  {"kind":"weekly_all","percent":13,"resets_at":"2026-10-29T07:00:00.697320+00:00","scope":null},
  {"kind":"weekly_scoped","percent":0,"resets_at":"2026-10-29T06:59:59.697480+00:00","scope":{"model":{"id":null,"display_name":"Fable"},"surface":null}}
 ]}
"""#
private let fiveOnly = #"{"five_hour":{"utilization":42,"resets_at":"2026-10-28T10:50:00Z"}}"#
private let fiveOnlyLimits = [L("5h", "5시간", 42, 1_793_184_600_000)]
private let realisticLimits = [L("5h", "5시간", 5, msFive), L("7d", "주간 · 전체", 13, msWeek), L("7d-Fable", "주간 · Fable", 0, msFable)]

// MARK: - 사용량 API 응답 해석

struct ParseRow: Row {
    let note: String
    let json: String
    fileprivate let expected: [L]?     // nil 이면 해석 실패
}

private let parseRows: [ParseRow] = [
    .init(note: "5시간만", json: fiveOnly, expected: fiveOnlyLimits),
    .init(note: "5시간 + 주간", json: #"{"five_hour":{"utilization":5,"resets_at":"2026-10-28T10:50:00Z"},"seven_day":{"utilization":13.5,"resets_at":"2026-10-29T07:00:00Z"}}"#,
          expected: [L("5h", "5시간", 5, 1_793_184_600_000), L("7d", "주간 · 전체", 13.5, 1_793_257_200_000)]),
    .init(note: "실제 응답 모양 — limits 의 session·weekly_all 은 보지 않고 weekly_scoped 만", json: realistic, expected: realisticLimits),
    .init(note: "모델별 주간 여러 개 — 응답 차례대로, 값은 percent", json: #"""
          {"five_hour":{"utilization":1},"limits":[
           {"kind":"weekly_scoped","percent":30,"scope":{"model":{"display_name":"Opus"}}},
           {"kind":"weekly_scoped","percent":70.5,"scope":{"model":{"display_name":"Fable"}}}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-Opus", "주간 · Opus", 30), L("7d-Fable", "주간 · Fable", 70.5)]),
    .init(note: "모델 이름이 없으면(scope null·model 없음·display_name null) \"모델\"", json: #"""
          {"five_hour":{"utilization":1},"limits":[{"kind":"weekly_scoped","percent":9,"scope":null}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-모델", "주간 · 모델", 9)]),
    .init(note: "display_name null 도 \"모델\"", json: #"""
          {"five_hour":{"utilization":1},"limits":[{"kind":"weekly_scoped","percent":9,"scope":{"model":{"display_name":null}}}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-모델", "주간 · 모델", 9)]),
    .init(note: "이름 없는 모델별 주간이 둘이면 뒤의 것 id 에 -2(라벨은 그대로) — 화면 목록의 id 가 겹치지 않게", json: #"""
          {"five_hour":{"utilization":1},"limits":[{"kind":"weekly_scoped","percent":1},{"kind":"weekly_scoped","percent":2}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-모델", "주간 · 모델", 1), L("7d-모델-2", "주간 · 모델", 2)]),
    .init(note: "같은 이름이 셋이면 -2·-3, 다른 이름 사이에 끼어도 차례대로", json: #"""
          {"five_hour":{"utilization":1},"limits":[
           {"kind":"weekly_scoped","percent":10,"scope":{"model":{"display_name":"Opus"}}},
           {"kind":"weekly_scoped","percent":20,"scope":{"model":{"display_name":"Fable"}}},
           {"kind":"weekly_scoped","percent":30,"scope":{"model":{"display_name":"Opus"}}},
           {"kind":"weekly_scoped","percent":40,"scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-Opus", "주간 · Opus", 10), L("7d-Fable", "주간 · Fable", 20),
                           L("7d-Opus-2", "주간 · Opus", 30), L("7d-Opus-3", "주간 · Opus", 40)]),
    .init(note: "붙인 id 가 다른 모델 이름과 겹치면 다음 수로(모델-2 라는 이름이 이미 있으면 -3)", json: #"""
          {"five_hour":{"utilization":1},"limits":[
           {"kind":"weekly_scoped","percent":1},
           {"kind":"weekly_scoped","percent":2,"scope":{"model":{"display_name":"모델-2"}}},
           {"kind":"weekly_scoped","percent":3}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-모델", "주간 · 모델", 1), L("7d-모델-2", "주간 · 모델-2", 2), L("7d-모델-3", "주간 · 모델", 3)]),
    .init(note: "빠진(숫자 아님) 항목은 id 를 차지하지 않는다 — 같은 이름의 앞 것이 빠지면 뒤의 것이 -2 없이", json: #"""
          {"five_hour":{"utilization":1},"limits":[
           {"kind":"weekly_scoped","percent":"x","scope":{"model":{"display_name":"Opus"}}},
           {"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: [L("5h", "5시간", 1), L("7d-Opus", "주간 · Opus", 3)]),
    .init(note: "weekly_scoped 의 utilization 은 보지 않는다(percent 만)", json: #"""
          {"five_hour":{"utilization":1},"limits":[{"kind":"weekly_scoped","utilization":50,"scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: [L("5h", "5시간", 1)]),
    .init(note: "five_hour 없음 → 실패", json: #"{"seven_day":{"utilization":13}}"#, expected: nil),
    .init(note: "five_hour 가 null → 실패", json: #"{"five_hour":null,"seven_day":{"utilization":13}}"#, expected: nil),
    .init(note: "five_hour 가 객체가 아님 → 실패", json: #"{"five_hour":5}"#, expected: nil),
    .init(note: "JSON 아님 → 실패", json: "not json", expected: nil),
    .init(note: "JSON 배열 → 실패", json: "[]", expected: nil),
    .init(note: "빈 글자 → 실패", json: "", expected: nil),
    .init(note: "숫자 아님은 그 항목만 빠짐(5시간 값이 글자)", json: #"{"five_hour":{"utilization":"5"},"seven_day":{"utilization":13}}"#,
          expected: [L("7d", "주간 · 전체", 13)]),
    .init(note: "숫자 아님은 그 항목만 빠짐(주간 null·모델별 percent 글자)", json: #"""
          {"five_hour":{"utilization":5},"seven_day":{"utilization":null},"limits":[{"kind":"weekly_scoped","percent":"x"},{"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: [L("5h", "5시간", 5), L("7d-Opus", "주간 · Opus", 3)]),
    .init(note: "five_hour 는 있으나 값이 숫자가 아니어서 푼 한도가 없음 → 실패", json: #"{"five_hour":{"utilization":null}}"#, expected: nil),
    .init(note: "five_hour 가 빈 객체라 푼 한도가 없음 → 실패", json: #"{"five_hour":{}}"#, expected: nil),
    .init(note: "푼 한도가 없음(주간도 숫자 아님·모델별 percent 없음) → 실패", json: #"""
          {"five_hour":{"utilization":null},"seven_day":{"utilization":"13"},"limits":[{"kind":"weekly_scoped","scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: nil),
    .init(note: "5시간은 못 풀어도 주간이 풀리면 그대로 성공", json: #"{"five_hour":{},"seven_day":{"utilization":13}}"#, expected: [L("7d", "주간 · 전체", 13)]),
    .init(note: "5시간은 못 풀어도 모델별 주간이 풀리면 그대로 성공", json: #"""
          {"five_hour":{"utilization":null},"limits":[{"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}}]}
          """#, expected: [L("7d-Opus", "주간 · Opus", 3)]),
    .init(note: "seven_day 가 객체가 아니면 그 항목만 빠짐", json: #"{"five_hour":{"utilization":5},"seven_day":"x"}"#, expected: [L("5h", "5시간", 5)]),
    .init(note: "limits 가 배열이 아니면 모델별 주간 없음", json: #"{"five_hour":{"utilization":5},"limits":{"kind":"weekly_scoped","percent":3}}"#,
          expected: [L("5h", "5시간", 5)]),
    .init(note: "limits 에 객체 아닌 것이 섞이면 그 원소만 빼고 객체인 원소는 본다", json: #"{"five_hour":{"utilization":5},"limits":[1,{"kind":"weekly_scoped","percent":3}]}"#,
          expected: [L("5h", "5시간", 5), L("7d-모델", "주간 · 모델", 3)]),
    .init(note: "객체 아닌 원소(null·글자·배열·수)가 여럿 섞여도 객체인 원소는 응답 차례대로", json: #"""
          {"five_hour":{"utilization":5},"limits":[null,{"kind":"weekly_scoped","percent":3,"scope":{"model":{"display_name":"Opus"}}},"x",[1],
           {"kind":"weekly_scoped","percent":4,"scope":{"model":{"display_name":"Fable"}}},2.5]}
          """#, expected: [L("5h", "5시간", 5), L("7d-Opus", "주간 · Opus", 3), L("7d-Fable", "주간 · Fable", 4)]),
    .init(note: "limits 가 객체 아닌 원소뿐이면 모델별 주간 없음", json: #"{"five_hour":{"utilization":5},"limits":[1,"x",null]}"#,
          expected: [L("5h", "5시간", 5)]),
    .init(note: "resets_at 소수 초 있음·없음·null·없음·깨짐", json: #"""
          {"five_hour":{"utilization":1,"resets_at":"2026-10-28T10:50:00.697299+00:00"},"seven_day":{"utilization":2,"resets_at":"2026-10-29T07:00:00+00:00"},
           "limits":[{"kind":"weekly_scoped","percent":3,"resets_at":null,"scope":{"model":{"display_name":"A"}}},
                     {"kind":"weekly_scoped","percent":4,"scope":{"model":{"display_name":"B"}}},
                     {"kind":"weekly_scoped","percent":5,"resets_at":"내일","scope":{"model":{"display_name":"C"}}}]}
          """#, expected: [L("5h", "5시간", 1, msFive), L("7d", "주간 · 전체", 2, 1_793_257_200_000),
                           L("7d-A", "주간 · A", 3), L("7d-B", "주간 · B", 4), L("7d-C", "주간 · C", 5)]),
]

@Suite("사용량: 응답 해석") struct UsageParseTests {
    @Test("five_hour 가 있고 푼 한도가 하나 이상이어야 성공, 항목마다 숫자가 아니면 그 항목만 뺀다", arguments: parseRows)
    func 표(_ row: ParseRow) {
        #expect(parseUsage(Data(row.json.utf8))?.map(L.init) == row.expected)
    }

    @Test("간략 이름 short·tiny", arguments: [
        ("5h", "5시간", "5시간", "5h"),
        ("7d", "주간 · 전체", "주간", "7d"),
        ("7d-Fable", "주간 · Fable", "Fable", "F"),
        ("7d-모델", "주간 · 모델", "모델", "모"),
        ("7d-A·B", "주간 · A·B", "B", "B"),          // 모델 이름에 · 가 있으면 마지막 조각
        ("7d-", "주간 · ", "", ""),                   // display_name 이 빈 글자
    ])
    func 간략_이름(id: String, label: String, short: String, tiny: String) {
        let l = Limit(id: id, label: label, pct: 0, resets: nil)
        #expect(l.short == short)
        #expect(l.tiny == tiny)
    }

    @Test("ISO8601 시각 — 소수 초 있음(밀리초까지)·없음·시간대·못 읽음", arguments: [
        ("2026-10-28T10:50:00.697299+00:00", Int64?.some(1_793_184_600_697)),
        ("2026-10-28T10:50:00.697Z", 1_793_184_600_697),
        ("2026-10-28T10:50:00.5Z", 1_793_184_600_500),
        ("2026-10-28T10:50:00Z", 1_793_184_600_000),
        ("2026-10-28T10:50:00+00:00", 1_793_184_600_000),
        ("2026-10-28T19:50:00+09:00", 1_793_184_600_000),
        ("2026-10-28", nil),
        ("내일", nil),
        ("", nil),
    ])
    func 시각(_ s: String, ms: Int64?) {
        #expect(isoDate(s).map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) } == ms)
    }

    @Test("시각이 없으면 nil") func 시각_없음() {
        #expect(isoDate(nil) == nil)
    }
}

// MARK: - 로그인 정보의 토큰 상태

enum TokenKind: Sendable { case missing, expired, valid }

struct TokenRow: Row {
    let note: String
    let json: String?          // 로그인 정보 JSON, nil 이면 키체인 항목 없음. "TOKEN" 자리에 가짜 토큰을 넣는다
    let expected: TokenKind
}

// 지금 u0 = 1970 기준 1,000,000초 = 1,000,000,000 밀리초
private let tokenRows: [TokenRow] = [
    .init(note: "키체인 항목 없음 → 없음", json: nil, expected: .missing),
    .init(note: "JSON 아님 → 없음", json: "not json", expected: .missing),
    .init(note: "JSON 배열 → 없음", json: "[]", expected: .missing),
    .init(note: "claudeAiOauth 없음 → 없음", json: #"{"other":{"accessToken":"TOKEN"}}"#, expected: .missing),
    .init(note: "accessToken 없음 → 없음", json: #"{"claudeAiOauth":{"expiresAt":2000000000000}}"#, expected: .missing),
    .init(note: "accessToken 이 글자가 아님 → 없음", json: #"{"claudeAiOauth":{"accessToken":123}}"#, expected: .missing),
    .init(note: "토큰이 없으면 만료를 보지 않는다(없음이 먼저)", json: #"{"claudeAiOauth":{"expiresAt":1}}"#, expected: .missing),
    .init(note: "expiresAt 없음 → 유효", json: #"{"claudeAiOauth":{"accessToken":"TOKEN"}}"#, expected: .valid),
    .init(note: "expiresAt 이 숫자가 아님 → 유효", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":"1"}}"#, expected: .valid),
    .init(note: "1밀리초 지남 → 만료", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":999999999}}"#, expected: .expired),
    .init(note: "같은 밀리초 → 아직 유효(지나야 만료)", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":1000000000}}"#, expected: .valid),
    .init(note: "1밀리초 남음 → 유효", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":1000000001}}"#, expected: .valid),
    .init(note: "한참 지남 → 만료", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":1}}"#, expected: .expired),
    .init(note: "소수 밀리초도 숫자로 본다", json: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":999999999.5}}"#, expected: .expired),
]

private func credentials(_ json: String?) -> Data? {
    data(json?.replacingOccurrences(of: "TOKEN", with: fakeToken))
}

@Suite("사용량: 로그인 정보의 토큰 상태") struct UsageTokenTests {
    @Test("없음·만료·유효", arguments: tokenRows)
    func 표(_ row: TokenRow) {
        let token = loginToken(credentials(row.json), now: u0)
        switch row.expected {
        case .missing: #expect(token == .missing)
        case .expired: #expect(token == .expired)
        case .valid: #expect(token == .valid(fakeToken))
        }
    }

    @Test("토큰 상태를 글자로 찍으면 토큰은 가린다") func 가림() {
        let token = LoginToken.valid(fakeToken)
        #expect(!String(describing: token).contains(fakeToken))
        #expect(!String(reflecting: token).contains(fakeToken))
        #expect(!"\(token)".contains(fakeToken))
    }
}

// MARK: - 캐시 확인

struct CacheRow: Row {
    let note: String
    let modified: Double?        // 캐시 수정 시각(지금에서 초), nil 이면 파일 없음
    let content: String?         // 내용, nil 이면 못 읽음
    var trustAfter: Double? = nil   // nil 이면 .distantPast(계정을 바꾼 적 없음)
    var fetching = false
    fileprivate let cached: [L]?    // 앱이 보일 한도, nil 이면 보이지 않음
    let fetch: Bool
    let read: Bool               // 내용을 읽었나 — 믿을 때만 읽는다
}

private let cacheRows: [CacheRow] = [
    .init(note: "캐시 없음 → 받는다", modified: nil, content: nil, cached: nil, fetch: true, read: false),
    .init(note: "30초 전 캐시 → 적용만 하고 받지 않는다", modified: -30, content: realistic, cached: realisticLimits, fetch: false, read: true),
    .init(note: "꼭 120초 → 받지 않는다(넘어야 받는다)", modified: -120, content: fiveOnly, cached: fiveOnlyLimits, fetch: false, read: true),
    .init(note: "120초 넘음 → 적용하고 받는다", modified: -120.5, content: fiveOnly, cached: fiveOnlyLimits, fetch: true, read: true),
    .init(note: "한참 지난 캐시 → 적용하고 받는다", modified: -86400, content: fiveOnly, cached: fiveOnlyLimits, fetch: true, read: true),
    .init(note: "계정 전환 뒤 trustAfter 전에 쓰인 캐시(상태줄이 늦게 씀) → 읽지도 않고 받는다",
          modified: -1, content: fiveOnly, trustAfter: 2, cached: nil, fetch: true, read: false),
    .init(note: "trustAfter 와 같은 시각 → 믿지 않는다", modified: 2, content: fiveOnly, trustAfter: 2, cached: nil, fetch: true, read: false),
    .init(note: "trustAfter 뒤에 쓰인 캐시 → 믿는다", modified: -5, content: fiveOnly, trustAfter: -10, cached: fiveOnlyLimits, fetch: false, read: true),
    .init(note: "받는 중 + 오래된 캐시 → 적용은 하되 받지 않는다", modified: -300, content: fiveOnly, fetching: true, cached: fiveOnlyLimits, fetch: false, read: true),
    .init(note: "받는 중 + 캐시 없음 → 받지 않는다", modified: nil, content: nil, fetching: true, cached: nil, fetch: false, read: false),
    .init(note: "깨진 캐시 + 10초 전 → 적용하지 않고, 믿을 캐시가 없는 것과 같게 받는다", modified: -10, content: "{\"five_", cached: nil, fetch: true, read: true),
    .init(note: "깨진 캐시 + 120초 넘음 → 받는다", modified: -200, content: "garbage", cached: nil, fetch: true, read: true),
    .init(note: "깨진 캐시 + 받는 중 → 받지 않는다", modified: -10, content: "garbage", fetching: true, cached: nil, fetch: false, read: true),
    .init(note: "five_hour 없는 캐시 → 적용하지 않고 받는다", modified: -30, content: #"{"seven_day":{"utilization":1}}"#, cached: nil, fetch: true, read: true),
    .init(note: "빈 한도 캐시(푼 한도 없음) + 10초 전 → 깨진 캐시와 같게 받는다", modified: -10, content: #"{"five_hour":{"utilization":null}}"#,
          cached: nil, fetch: true, read: true),
    .init(note: "수정 시각은 있으나 내용을 못 읽음(그 사이 지워짐) → 적용 없이 받는다", modified: -30, content: nil, cached: nil, fetch: true, read: true),
    .init(note: "수정 시각이 미래(시계 어긋남) + 풀리는 내용 → 적용하고 받는다", modified: 60, content: fiveOnly, cached: fiveOnlyLimits, fetch: true, read: true),
    .init(note: "수정 시각이 0.5초 미래 → 적용하고 받는다", modified: 0.5, content: fiveOnly, cached: fiveOnlyLimits, fetch: true, read: true),
    .init(note: "지금 막 쓰인 캐시(나이 0) → 적용만 하고 받지 않는다", modified: 0, content: fiveOnly, cached: fiveOnlyLimits, fetch: false, read: true),
    .init(note: "수정 시각이 미래 + 받는 중 → 적용하되 받지 않는다", modified: 60, content: fiveOnly, fetching: true, cached: fiveOnlyLimits, fetch: false, read: true),
    .init(note: "수정 시각이 미래 + 깨진 내용 → 받는다", modified: 60, content: "garbage", cached: nil, fetch: true, read: true),
]

@Suite("사용량: 캐시를 믿을지·새로 받을지") struct UsageCacheTests {
    @Test("trustAfter 뒤에 쓰인 캐시만 믿고, 믿을 캐시가 없거나 못 풀거나 수정 시각이 미래거나 120초 넘었으면 받는다(받는 중이면 받지 않는다)",
          arguments: cacheRows)
    func 표(_ row: CacheRow) {
        let cache = FakeUsageCache()
        cache.modified = row.modified.map { u0 + $0 }
        cache.content = data(row.content)
        let check = checkUsageCache(cache: cache, clock: FixedClock(now: u0),
                                    trustAfter: row.trustAfter.map { u0 + $0 } ?? .distantPast, fetching: row.fetching)
        #expect(check.cached?.map(L.init) == row.cached)
        #expect(check.fetch == row.fetch)
        #expect(cache.calls == (row.read ? ["modified", "read"] : ["modified"]))   // 확인은 쓰거나 지우지 않는다
    }
}

// MARK: - 받기

struct FetchRow: Row {
    let note: String
    let credentials: String?     // 로그인 정보 JSON("TOKEN" 자리에 가짜 토큰), nil 이면 항목 없음
    let reply: String?           // API 가 200 으로 준 본문, nil 이면 200 아님
    let called: Bool             // API 를 불렀나
    let expired: Bool
}

private let validLogin = #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":2000000000000}}"#

private let fetchRows: [FetchRow] = [
    .init(note: "로그인 정보 없음 → 부르지 않는다", credentials: nil, reply: fiveOnly, called: false, expired: false),
    .init(note: "JSON 아님 → 부르지 않는다", credentials: "garbage", reply: fiveOnly, called: false, expired: false),
    .init(note: "accessToken 없음 → 부르지 않는다", credentials: #"{"claudeAiOauth":{}}"#, reply: fiveOnly, called: false, expired: false),
    .init(note: "만료 → 부르지 않고 tokenExpired", credentials: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":999999999}}"#,
          reply: fiveOnly, called: false, expired: true),
    .init(note: "만료 경계(같은 밀리초) → 부른다", credentials: #"{"claudeAiOauth":{"accessToken":"TOKEN","expiresAt":1000000000}}"#,
          reply: fiveOnly, called: true, expired: false),
    .init(note: "유효 + 200 → 받은 본문 그대로(여기서 풀지 않는다)", credentials: validLogin, reply: "not usage json", called: true, expired: false),
    .init(note: "유효 + 200 아님 → 본문 없음", credentials: validLogin, reply: nil, called: true, expired: false),
]

@Suite("사용량: 받기") struct UsageFetchTests {
    @Test("토큰이 유효할 때만 API 를 부른다", arguments: fetchRows)
    func 표(_ row: FetchRow) {
        let login = FakeCredentials(), api = FakeUsageAPI()
        login.raw = credentials(row.credentials)
        api.reply = data(row.reply)
        let fetched = fetchUsage(credentials: login, api: api, clock: FixedClock(now: u0))
        #expect(login.reads == 1)
        #expect(api.tokens.count == (row.called ? 1 : 0))
        #expect(api.tokens.allSatisfy { $0 == fakeToken }, "API 에 넘긴 토큰이 로그인 정보의 토큰이 아니다")
        #expect(fetched == UsageFetch(data: row.called ? data(row.reply) : nil, tokenExpired: row.expired))
    }
}

// MARK: - 받은 결과 처리

struct SettleRow: Row {
    let note: String
    let data: String?            // 받은 본문(200), nil 이면 200 아님·부르지 않음
    var expired = false
    let shown: Bool              // 앞서 보이던 한도가 있나
    fileprivate let limits: [L]?    // 새로 보일 한도, nil 이면 그대로
    let failed: Bool?            // nil 이면 건드리지 않는다
    let written: Bool            // 받은 본문을 그대로 캐시에 썼나 — 이때 trustAfter 도 푼다
}

private let settleRows: [SettleRow] = [
    .init(note: "200 + 풀림 → 적용·캐시에 그대로 쓰기·trustAfter 풀기·failed 풀기", data: realistic, shown: false,
          limits: realisticLimits, failed: false, written: true),
    .init(note: "200 + 풀림, 앞서 보이던 한도 있음 → 같다", data: fiveOnly, shown: true, limits: fiveOnlyLimits, failed: false, written: true),
    .init(note: "200 아님 + 한도 비었음 → failed", data: nil, shown: false, limits: nil, failed: true, written: false),
    .init(note: "200 아님 + 한도 있음 → failed 그대로", data: nil, shown: true, limits: nil, failed: nil, written: false),
    .init(note: "200 이지만 해석 실패 + 한도 비었음 → failed, 캐시에 쓰지 않는다", data: #"{"error":"x"}"#, shown: false, limits: nil, failed: true, written: false),
    .init(note: "200 이지만 해석 실패 + 한도 있음 → 그대로", data: "not json", shown: true, limits: nil, failed: nil, written: false),
    .init(note: "만료 + 한도 비었음 → tokenExpired, failed", data: nil, expired: true, shown: false, limits: nil, failed: true, written: false),
    .init(note: "만료 + 한도 있음 → tokenExpired, failed 그대로", data: nil, expired: true, shown: true, limits: nil, failed: nil, written: false),
    .init(note: "빈 한도 응답(값이 숫자 아님) + 한도 비었음 → failed, 캐시에 쓰지 않는다",
          data: #"{"five_hour":{"utilization":null}}"#, shown: false, limits: nil, failed: true, written: false),
    .init(note: "빈 한도 응답(값이 숫자 아님) + 한도 있음 → 그대로, 캐시에 쓰지 않는다",
          data: #"{"five_hour":{"utilization":null}}"#, shown: true, limits: nil, failed: nil, written: false),
    .init(note: "빈 한도 응답(five_hour 빈 객체) + 한도 비었음 → failed, 캐시에 쓰지 않는다",
          data: #"{"five_hour":{}}"#, shown: false, limits: nil, failed: true, written: false),
    .init(note: "빈 한도 응답(five_hour 빈 객체) + 한도 있음 → 그대로, 캐시에 쓰지 않는다",
          data: #"{"five_hour":{}}"#, shown: true, limits: nil, failed: nil, written: false),
    .init(note: "5시간은 못 풀어도 주간이 풀리면 성공 → 적용·캐시에 쓰기",
          data: #"{"five_hour":{"utilization":null},"seven_day":{"utilization":13}}"#, shown: false, limits: [L("7d", "주간 · 전체", 13)], failed: false, written: true),
]

@Suite("사용량: 받은 결과 처리") struct UsageSettleTests {
    @Test("200 이고 풀리면 적용·캐시 쓰기·trustAfter 풀기, 아니면 한도가 비었을 때만 failed", arguments: settleRows)
    func 표(_ row: SettleRow) {
        let cache = FakeUsageCache()
        let trustAfter = u0 + 5                       // 계정을 막 바꾼 뒤
        let current = row.shown ? [Limit(id: "5h", label: "5시간", pct: 1, resets: nil)] : []
        let body = data(row.data)
        let settled = settleUsage(UsageFetch(data: body, tokenExpired: row.expired), current: current, trustAfter: trustAfter, cache: cache)
        #expect(settled.limits?.map(L.init) == row.limits)
        #expect(settled.failed == row.failed)
        #expect(settled.tokenExpired == row.expired)
        #expect(settled.trustAfter == (row.written ? .distantPast : trustAfter))
        #expect(cache.written == (row.written ? [body!] : []))
        #expect(cache.calls == (row.written ? ["write"] : []))
    }
}

// MARK: - 흐름 (앱의 refresh 처럼 확인 → 받기 → 처리)

@Suite("사용량: 계정 전환 뒤 흐름") struct UsageFlowTests {
    @Test("늦게 쓰인 앞 계정 캐시는 무시하고 새로 받아 쓴 뒤로는 다시 믿는다") func 계정_전환() {
        let cache = FakeUsageCache(), login = FakeCredentials(), api = FakeUsageAPI()
        login.raw = credentials(validLogin)
        // 계정을 바꿈(u0): 앱이 trustAfter 를 5초 뒤로 두고 캐시를 지운다
        var trustAfter = u0 + 5
        cache.remove()
        // 상태줄이 앞 계정 토큰으로 받던 값을 1초 뒤에 늦게 써 넣는다
        cache.modified = u0 + 1
        cache.content = data(fiveOnly)
        // 2초 뒤 확인: 그 캐시는 믿지 않고 받는다
        var check = checkUsageCache(cache: cache, clock: FixedClock(now: u0 + 2), trustAfter: trustAfter, fetching: false)
        #expect(check.cached == nil)
        #expect(check.fetch)
        // 새 계정 토큰으로 받아 3초에 캐시에 쓴다 — trustAfter(5초) 전이지만 직접 받은 값이라 푼다
        api.reply = data(realistic)
        cache.writeAt = u0 + 3
        let fetched = fetchUsage(credentials: login, api: api, clock: FixedClock(now: u0 + 2))
        let settled = settleUsage(fetched, current: [], trustAfter: trustAfter, cache: cache)
        trustAfter = settled.trustAfter
        #expect(settled.limits?.map(L.init) == realisticLimits)
        #expect(settled.failed == false)
        #expect(cache.written == [data(realistic)!])
        // 30초 뒤 확인: 방금 쓴 캐시를 믿고 적용만 한다
        check = checkUsageCache(cache: cache, clock: FixedClock(now: u0 + 33), trustAfter: trustAfter, fetching: false)
        #expect(check.cached?.map(L.init) == realisticLimits)
        #expect(!check.fetch)
        #expect(api.tokens.count == 1)
    }

    @Test("상태줄이 못 읽는 내용을 계속 쓰면 확인마다 받고, 받아 쓴 뒤로는 믿는다") func 깨진_캐시() {
        let cache = FakeUsageCache(), login = FakeCredentials(), api = FakeUsageAPI()
        login.raw = credentials(validLogin)
        // 상태줄이 5초 전에 깨진 내용을 썼다 — 확인할 때마다(앱은 30초마다) 받는다
        cache.modified = u0 - 5
        cache.content = data("{\"five_")
        for t in [0.0, 30, 60] {
            let check = checkUsageCache(cache: cache, clock: FixedClock(now: u0 + t), trustAfter: .distantPast, fetching: false)
            #expect(check.cached == nil)
            #expect(check.fetch, "\(t)초")
            cache.modified = u0 + t + 1          // 받는 동안 상태줄이 또 깨진 내용을 쓴다
        }
        // 받은 응답이 풀리면 캐시에 그대로 쓰고, 다음 확인부터는 그 캐시를 믿어 받지 않는다
        api.reply = data(fiveOnly)
        cache.writeAt = u0 + 62
        let settled = settleUsage(fetchUsage(credentials: login, api: api, clock: FixedClock(now: u0 + 61)), current: [], trustAfter: .distantPast, cache: cache)
        #expect(settled.limits?.map(L.init) == fiveOnlyLimits)
        let check = checkUsageCache(cache: cache, clock: FixedClock(now: u0 + 90), trustAfter: .distantPast, fetching: false)
        #expect(check.cached?.map(L.init) == fiveOnlyLimits)
        #expect(!check.fetch)
    }
}
