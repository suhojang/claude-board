// 계정 전환 규칙 시험 — 키체인·~/.claude.json·accounts.json·쓰기 잠금·시계를 가짜 어댑터로 넣어 실제 키체인·파일 없이 돌린다.
// 가짜는 이 파일에만 둔다(Sources/BoardRules 에 두지 않는다). 가짜는 부른 것을 한 줄씩 적어 두어 잠금 안팎의 호출 순서까지 확인한다.
// 기대값은 AccountStore 에 있던 지금 동작 그대로다(버그 의심도 "지금 동작" 으로 적고 고치지 않는다).
// 옮긴 뒤 고친 것(깨진 accounts.json 을 다음 저장이 덮어쓰던 것, 이슈 #9 3차의 요금제 글자·저장 계정 지우기의 결과·
// 지금 계정 정보가 모자라 전환을 막을 때의 안내·지금 계정 정보가 아예 없을 때 로그인 토큰을 덮어쓰던 것)은 고친 동작을 기대값으로 적는다.
// 시험의 토큰·이메일은 가짜 글자다 — 진짜 토큰이 아니고, note·#expect 메시지에 토큰을 넣지 않는다
import Foundation
import Testing
import BoardRules

private let a0 = Date(timeIntervalSince1970: 2_000_000)    // 시험의 지금

// MARK: - 가짜 어댑터 (시험마다 새로 만들고 한 스레드에서만 쓴다 — 포트가 Sendable 이라 @unchecked)

// 여러 가짜가 함께 쓰는 호출 기록
private final class CallLog: @unchecked Sendable {
    var calls: [String] = []
}

private let loginItem = KeychainItem(service: "Claude Code-credentials", account: "tester")

private func itemName(_ item: KeychainItem) -> String {
    if item == loginItem { return "login" }
    if item.service == KeychainItem.savedAccountService { return "vault:\(item.account)" }
    return item.description
}

private final class FakeKeychain: KeychainItems, @unchecked Sendable {
    let log: CallLog
    var items: [KeychainItem: Data] = [:]
    var unreadable: Set<KeychainItem> = []      // 항목은 있는데 읽지 못한다
    var failWrite: Set<KeychainItem> = []
    var failDelete: Set<KeychainItem> = []      // 지우기에 실패한다(항목이 남는다)
    init(log: CallLog) { self.log = log }
    func read(_ item: KeychainItem) -> Data? {
        log.calls.append("read \(itemName(item))")
        return unreadable.contains(item) ? nil : items[item]
    }
    func exists(_ item: KeychainItem) -> Bool {
        log.calls.append("exists \(itemName(item))")
        return items[item] != nil || unreadable.contains(item)
    }
    func write(_ item: KeychainItem, json: [String: Any]) throws -> Bool {
        log.calls.append("write \(itemName(item))")
        guard !failWrite.contains(item) else { return false }
        items[item] = try JSONSerialization.data(withJSONObject: json, options: [.sortedKeys])
        unreadable.remove(item)
        return true
    }
    func delete(_ item: KeychainItem) {
        log.calls.append("delete \(itemName(item))")
        guard !failDelete.contains(item) else { return }
        items[item] = nil
        unreadable.remove(item)
    }
    func json(_ item: KeychainItem) -> [String: Any]? {
        items[item].flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }
}

private final class FakeConfig: ClaudeConfig, @unchecked Sendable {
    let log: CallLog
    var json: [String: Any]?                    // nil 이면 파일 없음
    var patchError: Error?
    init(log: CallLog) { self.log = log }
    func oauthAccount() -> [String: Any]? {
        log.calls.append("config read")
        return json?["oauthAccount"] as? [String: Any]
    }
    func patch(_ change: (inout [String: Any]) -> Void) throws {
        log.calls.append("config patch")
        if let patchError { throw patchError }
        guard var j = json else { throw AccountError(".claude.json 를 읽지 못했습니다") }   // 진짜 어댑터(patchJSON)와 같은 글자
        change(&j)
        json = j
    }
}

private final class FakeSaved: SavedAccountList, @unchecked Sendable {
    let log: CallLog
    var data: Data?
    var writeError: Error?
    var setAsideError: Error?
    private(set) var aside: [Data?] = []        // 옆으로 치운 내용(차례대로)
    init(log: CallLog) { self.log = log }
    func read() -> Data? { log.calls.append("saved read"); return data }
    func write(_ data: Data) throws {
        log.calls.append("saved write")
        if let writeError { throw writeError }
        self.data = data
    }
    // 진짜 어댑터처럼 파일을 옮긴다 — 옮긴 뒤에는 파일이 없다. 시각은 기록에 1970 기준 초로 남긴다
    func setAside(at: Date) throws {
        log.calls.append("saved set-aside \(at.timeIntervalSince1970)")
        if let setAsideError { throw setAsideError }
        aside.append(data)
        data = nil
    }
}

private final class FakeLock: CredentialsWriteLock, @unchecked Sendable {
    let log: CallLog
    var error: Error?
    init(log: CallLog) { self.log = log }
    func withLock<T>(_ body: () throws -> T) throws -> T {
        if let error { log.calls.append("lock failed"); throw error }
        log.calls.append("lock")
        defer { log.calls.append("unlock") }
        return try body()
    }
}

private struct FixedClock: Clock { let now: Date }

// 가짜 바깥 한 벌
private final class World {
    let log = CallLog()
    let keychain: FakeKeychain
    let config: FakeConfig
    let saved: FakeSaved
    let lock: FakeLock
    init() {
        keychain = FakeKeychain(log: log)
        config = FakeConfig(log: log)
        saved = FakeSaved(log: log)
        lock = FakeLock(log: log)
    }
    var ports: AccountPorts {
        AccountPorts(keychain: keychain, login: loginItem, config: config, saved: saved, writeLock: lock, clock: FixedClock(now: a0))
    }
    var savedEntries: [[String: Any]] { savedAccountEntries(saved.data) }
}

private struct TestError: LocalizedError {
    let errorDescription: String?
    init(_ m: String) { errorDescription = m }
}

// 깨진 저장 계정 목록을 옆으로 치우지 못했을 때의 안내 — 사용자에게 보이는 글자라 긴 줄표 없이
private let setAsideFailed = "저장 계정 목록(accounts.json)을 읽지 못해 옆으로 옮기려 했지만 옮기지 못했습니다"
// 지우기의 결과 안내 — 키체인에서 못 지움(던짐, 아무것도 바꾸지 않음), 목록이 깨짐·목록을 못 씀(돌려줌, 토큰은 지웠다)
private let removeKeychainFailed = "키체인의 로그인 정보를 지우지 못해 목록에 그대로 두었습니다. 잠시 뒤 다시 지워 주세요"
private let removeListBroken = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 읽지 못해 목록은 그대로 두었습니다"
private let removeListWriteFailed = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: "
// 지금 로그인된 계정의 정보에 accountUuid 가 없어 전환을 막을 때의 안내(앱이 "전환하지 못했습니다: " 뒤에 붙인다)
private let currentIncomplete = "지금 로그인된 계정의 정보가 모자라 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요"
// 지금 로그인된 계정의 정보(oauthAccount)가 아예 없어 전환을 막을 때의 안내 — 위와 끝이 같고 까닭만 "없어"
private let currentMissing = "지금 로그인된 계정의 정보가 없어 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요"

// MARK: - 시험 자료 (모두 가짜)

// 가짜 계정 정보 — id 는 "<uuid>|org"
private func oauthAccount(_ uuid: String) -> [String: Any] {
    ["accountUuid": uuid, "organizationUuid": "org", "emailAddress": "\(uuid.lowercased())@example.com",
     "organizationName": "Org \(uuid)", "organizationType": "claude_max", "organizationRateLimitTier": "default_claude_max_5x"]
}
private func id(_ uuid: String) -> String { "\(uuid)|org" }
private func account(_ uuid: String) -> Account { Account(oauth: oauthAccount(uuid))! }

// 가짜 토큰 묶음(claudeAiOauth) — 이름표로만 견준다
private func token(_ name: String, refreshExpires: Int64? = 2_100_000_000_000) -> [String: Any] {
    var t: [String: Any] = ["accessToken": "fake-access-\(name)", "refreshToken": "fake-refresh-\(name)", "expiresAt": 2_000_000_000_000]
    if let refreshExpires { t["refreshTokenExpiresAt"] = refreshExpires }
    return t
}
private func tokenName(_ credentials: [String: Any]?) -> String? {
    ((credentials?["claudeAiOauth"] as? [String: Any])?["accessToken"] as? String)?.replacingOccurrences(of: "fake-access-", with: "")
}
private func data(_ obj: Any) -> Data { try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]) }
private func json(_ s: String) -> [String: Any] { try! JSONSerialization.jsonObject(with: Data(s.utf8)) as! [String: Any] }

// MARK: - 계정 모델

struct AccountRow: Row {
    let note: String
    let oauth: String          // oauthAccount JSON
    let id: String?            // nil 이면 계정이 아니다
    var email = "?"
    var org = ""
    var plan = ""
}

private let accountRows: [AccountRow] = [
    .init(note: "id 는 accountUuid|organizationUuid, Max 5x",
          oauth: #"{"accountUuid":"u1","organizationUuid":"o1","emailAddress":"a@example.com","organizationName":"가짜 조직","organizationType":"claude_max","organizationRateLimitTier":"default_claude_max_5x"}"#,
          id: "u1|o1", email: "a@example.com", org: "가짜 조직", plan: "Max 5x"),
    .init(note: "organizationUuid 없음 → 빈 글자로 붙인다", oauth: #"{"accountUuid":"u1"}"#, id: "u1|"),
    .init(note: "organizationUuid 가 글자가 아님 → 빈 글자", oauth: #"{"accountUuid":"u1","organizationUuid":7}"#, id: "u1|"),
    .init(note: "accountUuid 없음 → 계정 아님", oauth: #"{"organizationUuid":"o1","emailAddress":"a@example.com"}"#, id: nil),
    .init(note: "accountUuid 빈 글자 → 계정 아님", oauth: #"{"accountUuid":"","organizationUuid":"o1"}"#, id: nil),
    .init(note: "accountUuid 가 글자가 아님 → 계정 아님", oauth: #"{"accountUuid":12}"#, id: nil),
    .init(note: "이메일·조직 이름 없음 → ?·빈 글자, 요금제 없음 → 빈 글자", oauth: #"{"accountUuid":"u1","organizationUuid":"o1"}"#, id: "u1|o1"),
    .init(note: "Max 20x", oauth: #"{"accountUuid":"u","organizationType":"claude_max","organizationRateLimitTier":"default_claude_max_20x"}"#,
          id: "u|", plan: "Max 20x"),
    .init(note: "등급 없음 → Pro", oauth: #"{"accountUuid":"u","organizationType":"claude_pro"}"#, id: "u|", plan: "Pro"),
    .init(note: "등급 글자가 _<수>x 로 끝나지 않으면 붙이지 않는다", oauth: #"{"accountUuid":"u","organizationType":"claude_max","organizationRateLimitTier":"default_claude_ai"}"#,
          id: "u|", plan: "Max"),
    .init(note: "등급 끝이 아닌 _5x 는 보지 않는다", oauth: #"{"accountUuid":"u","organizationType":"claude_team","organizationRateLimitTier":"tier_5x_beta"}"#,
          id: "u|", plan: "Team"),
    .init(note: "claude_ 가 없는 종류는 첫 글자만 대문자", oauth: #"{"accountUuid":"u","organizationType":"enterprise"}"#, id: "u|", plan: "Enterprise"),
    .init(note: "등급이 글자가 아님 → 붙이지 않는다", oauth: #"{"accountUuid":"u","organizationType":"claude_max","organizationRateLimitTier":5}"#,
          id: "u|", plan: "Max"),
    .init(note: "종류 없이 등급만 _5x 면 \"5x\"(비지 않은 조각만 빈칸으로 잇는다 — 앞에 빈칸이 붙지 않는다)",
          oauth: #"{"accountUuid":"u","organizationRateLimitTier":"default_claude_max_5x"}"#, id: "u|", plan: "5x"),
    .init(note: "종류가 claude_ 뿐(떼면 빈 글자)이고 등급만 있어도 \"20x\"",
          oauth: #"{"accountUuid":"u","organizationType":"claude_","organizationRateLimitTier":"default_claude_max_20x"}"#, id: "u|", plan: "20x"),
]

@Suite("계정 전환: 계정 모델") struct AccountModelTests {
    @Test("id·이메일·조직·요금제 글자", arguments: accountRows)
    func 표(_ row: AccountRow) {
        let a = Account(oauth: json(row.oauth))
        #expect(a?.id == row.id)
        if let a {
            #expect(a.email == row.email)
            #expect(a.org == row.org)
            #expect(a.plan == row.plan)
        }
    }

    @Test("다시 로그인해야 하나 — refreshExpires 가 지나야 만료(같은 시각은 아직), 없으면 만료 아님", arguments: [
        (Double?.none, false), (-1, true), (-0.001, true), (0, false), (0.001, false), (86400, false),
    ])
    func 만료(_ offset: Double?, expired: Bool) {
        let a = Account(oauth: oauthAccount("A"), refreshExpires: offset.map { a0 + $0 })!
        #expect(a.isExpired(at: a0) == expired)
    }

    @Test("같음은 화면에 보이는 값(id·이메일·조직·요금제·만료)만 견준다") func 같음() {
        var other = oauthAccount("A")
        other["displayName"] = "다른 이름"
        #expect(Account(oauth: oauthAccount("A")) == Account(oauth: other))
        #expect(Account(oauth: oauthAccount("A"), refreshExpires: a0) != Account(oauth: oauthAccount("A"), refreshExpires: a0 + 1))
        #expect(Account(oauth: oauthAccount("A"), refreshExpires: a0) == Account(oauth: oauthAccount("A"), refreshExpires: a0))
        var mail = oauthAccount("A")
        mail["emailAddress"] = "other@example.com"
        #expect(Account(oauth: oauthAccount("A")) != Account(oauth: mail))
        var plan = oauthAccount("A")
        plan["organizationRateLimitTier"] = "default_claude_max_20x"
        #expect(Account(oauth: oauthAccount("A")) != Account(oauth: plan))
        var org = oauthAccount("A")
        org["organizationUuid"] = "org2"
        #expect(Account(oauth: oauthAccount("A")) != Account(oauth: org))
    }
}

// MARK: - 저장 계정 목록 (accounts.json)

struct SavedParseRow: Row {
    let note: String
    let file: String?            // 파일 내용, nil 이면 파일 없음
    let expected: [String]       // "<id>" 또는 "<id>@<만료 밀리초>"
}

private let savedParseRows: [SavedParseRow] = [
    .init(note: "파일 없음 → 빈 목록", file: nil, expected: []),
    .init(note: "JSON 아님 → 빈 목록", file: "not json", expected: []),
    .init(note: "JSON 배열 → 빈 목록", file: "[]", expected: []),
    .init(note: "accounts 없음 → 빈 목록", file: #"{"other":[]}"#, expected: []),
    .init(note: "accounts 가 배열이 아님 → 빈 목록", file: #"{"accounts":{"oauthAccount":{"accountUuid":"A"}}}"#, expected: []),
    .init(note: "파일 차례대로, 만료는 밀리초", file: #"""
          {"accounts":[{"oauthAccount":{"accountUuid":"A","organizationUuid":"org"},"savedAt":1,"refreshTokenExpiresAt":2100000000000},
                       {"oauthAccount":{"accountUuid":"B","organizationUuid":"org"},"savedAt":2}]}
          """#, expected: ["A|org@2100000000000", "B|org"]),
    .init(note: "oauthAccount 없는 항목·accountUuid 없는 항목은 뺀다", file: #"""
          {"accounts":[{"savedAt":1},{"oauthAccount":"x"},{"oauthAccount":{"emailAddress":"x@example.com"}},{"oauthAccount":{"accountUuid":"B","organizationUuid":"org"}}]}
          """#, expected: ["B|org"]),
    .init(note: "만료가 숫자가 아니면 없음, 소수 밀리초도 숫자", file: #"""
          {"accounts":[{"oauthAccount":{"accountUuid":"A"},"refreshTokenExpiresAt":"2100000000000"},{"oauthAccount":{"accountUuid":"B"},"refreshTokenExpiresAt":1500.5}]}
          """#, expected: ["A|", "B|@1500.5"]),
    .init(note: "객체가 아닌 항목이 하나라도 섞이면 표시는 빈 목록(쓰기 길은 깨짐으로 보고 옆으로 치운다 — 아래 쓰기 해석)",
          file: #"{"accounts":[{"oauthAccount":{"accountUuid":"A"}},1]}"#, expected: []),
]

// 쓰기 길(저장·지우기)의 해석 — 비었으면 빈 목록에서 시작, 객체 배열은 그대로, 그 밖은 깨짐(nil)
struct WritableParseRow: Row {
    let note: String
    let file: String?            // 파일 내용, nil 이면 파일 없음
    let count: Int?              // 항목 수, nil 이면 깨짐
}

// 깨진 꼴 — 저장은 옆으로 치우고 새 목록으로, 지우기는 목록 파일을 건드리지 않는다
private let brokenSavedFiles: [String] = [
    "not json",
    #"{"accounts":[{"oauthAccount":{"accountUuid":"B","organizationUuid":"org"}}"#,
    "[]",
    #"{"accounts":{"oauthAccount":{"accountUuid":"B","organizationUuid":"org"}}}"#,
    #"{"accounts":null}"#,
    #"{"accounts":[{"oauthAccount":{"accountUuid":"B","organizationUuid":"org"}},"broken"]}"#,
]

// 빈 꼴 — 옆으로 치우지 않고 빈 목록에서 쓴다
private let emptySavedFiles: [String?] = [nil, "", " \n\t\r ", "{}", #"{"other":1}"#]

private let writableParseRows: [WritableParseRow] = [
    .init(note: "파일 없음 → 빈 목록", file: nil, count: 0),
    .init(note: "빈 바이트 → 빈 목록", file: "", count: 0),
    .init(note: "JSON 공백만 → 빈 목록", file: " \n\t\r ", count: 0),
    .init(note: "{} → 빈 목록", file: "{}", count: 0),
    .init(note: "accounts 키 없음 → 빈 목록(다른 키는 다음 쓰기에서 빠진다)", file: #"{"other":1}"#, count: 0),
    .init(note: "빈 accounts → 빈 목록", file: #"{"accounts":[]}"#, count: 0),
    .init(note: "객체 배열은 그대로 — id 를 못 만드는 객체도 남긴다", file: #"""
          {"accounts":[{"oauthAccount":{"accountUuid":"A","organizationUuid":"org"}},{"savedAt":1},{}]}
          """#, count: 3),
    .init(note: "JSON 아님 → 깨짐", file: "not json", count: nil),
    .init(note: "중간에 잘린 JSON → 깨짐", file: #"{"accounts":[{"oauthAccount":{"accountUuid":"B"}}"#, count: nil),
    .init(note: "최상위가 배열 → 깨짐", file: "[]", count: nil),
    .init(note: "최상위가 글자 → 깨짐", file: #""accounts""#, count: nil),
    .init(note: "accounts 가 객체 → 깨짐", file: #"{"accounts":{"oauthAccount":{"accountUuid":"B"}}}"#, count: nil),
    .init(note: "accounts 가 글자 → 깨짐", file: #"{"accounts":"x"}"#, count: nil),
    .init(note: "accounts 가 null → 깨짐(있지만 배열이 아님)", file: #"{"accounts":null}"#, count: nil),
    .init(note: "객체가 아닌 항목이 섞임 → 일부 살리기가 아니라 깨짐", file: #"{"accounts":[{"oauthAccount":{"accountUuid":"A"}},1]}"#, count: nil),
    .init(note: "배열 항목이 섞임 → 깨짐", file: #"{"accounts":[[]]}"#, count: nil),
]

private func describe(_ a: Account) -> String {
    a.refreshExpires.map { e in
        let ms = (e.timeIntervalSince1970 * 10_000).rounded() / 10   // 밀리초를 소수 한 자리까지(초로 나눴다 곱하며 생기는 오차를 지운다)
        return "\(a.id)@" + (ms == ms.rounded() ? String(Int64(ms)) : String(ms))
    } ?? a.id
}

@Suite("계정 전환: 저장 계정 목록") struct SavedAccountListTests {
    @Test("파일 → 계정", arguments: savedParseRows)
    func 해석(_ row: SavedParseRow) {
        let saved = FakeSaved(log: CallLog())
        saved.data = row.file.map { Data($0.utf8) }
        #expect(loadSavedAccounts(saved).map(describe) == row.expected)
    }

    @Test("쓰기 길의 해석 — 비었으면 빈 목록, 객체 배열은 그대로, 그 밖은 깨짐", arguments: writableParseRows)
    func 쓰기_해석(_ row: WritableParseRow) {
        #expect(writableSavedAccountEntries(row.file.map { Data($0.utf8) })?.count == row.count)
    }

    @Test("쓰기 길의 해석 — UTF-8 이 아닌 바이트는 깨짐") func 쓰기_해석_바이트() {
        #expect(writableSavedAccountEntries(Data([0xFF, 0xFE, 0x00])) == nil)
    }

    @Test("항목의 계정 id") func 항목_id() {
        #expect(savedAccountID(["oauthAccount": oauthAccount("A")]) == "A|org")
        #expect(savedAccountID(["oauthAccount": ["emailAddress": "x@example.com"]]) == nil)
        #expect(savedAccountID(["savedAt": 1]) == nil)
    }

    @Test("같은 id 는 제자리에서(처음 것만) 바꾸고, 없으면 끝에 더한다") func 갱신() {
        let list: [[String: Any]] = [["oauthAccount": oauthAccount("A"), "n": 1], ["savedAt": 0], ["oauthAccount": oauthAccount("B"), "n": 2],
                                     ["oauthAccount": oauthAccount("A"), "n": 3]]
        let replaced = upsertingSavedAccount(list, ["oauthAccount": oauthAccount("A"), "n": 9], id: id("A"))
        #expect(replaced.map { $0["n"] as? Int } == [9, nil, 2, 3])
        let added = upsertingSavedAccount(list, ["oauthAccount": oauthAccount("C"), "n": 4], id: id("C"))
        #expect(added.map { $0["n"] as? Int } == [1, nil, 2, 3, 4])
        #expect(upsertingSavedAccount([], ["n": 5], id: "x").map { $0["n"] as? Int } == [5])
    }

    @Test("지우기는 그 id 항목을 모두 빼고, id 를 못 만드는 항목은 남긴다") func 지우기() {
        let list: [[String: Any]] = [["oauthAccount": oauthAccount("A"), "n": 1], ["savedAt": 0], ["oauthAccount": oauthAccount("B"), "n": 2],
                                     ["oauthAccount": oauthAccount("A"), "n": 3]]
        #expect(removingSavedAccount(list, id: id("A")).map { $0["n"] as? Int } == [nil, 2])
        #expect(removingSavedAccount(list, id: id("Z")).count == 4)
    }

    @Test("저장할 항목 — 보일 정보·저장 시각(초)·토큰의 refreshTokenExpiresAt 만(토큰은 넣지 않는다)") func 항목() {
        let e = savedAccountEntry(oauthAccount: oauthAccount("A"), credentials: token("A"), savedAt: a0)
        #expect(Set(e.keys) == ["oauthAccount", "savedAt", "refreshTokenExpiresAt"])
        #expect(e["savedAt"] as? Double == a0.timeIntervalSince1970)
        #expect((e["refreshTokenExpiresAt"] as? NSNumber)?.int64Value == 2_100_000_000_000)
        #expect(savedAccountID(e) == id("A"))
        let noRefresh = savedAccountEntry(oauthAccount: oauthAccount("A"), credentials: token("A", refreshExpires: nil), savedAt: a0)
        #expect(Set(noRefresh.keys) == ["oauthAccount", "savedAt"])
    }

    @Test("파일 형식 — {\"accounts\": [...]} 를 줄 나눠 쓰고 / 는 그대로, 다시 읽으면 같다") func 형식() throws {
        let list: [[String: Any]] = [["oauthAccount": ["accountUuid": "A", "organizationUuid": "org", "emailAddress": "a/b@example.com"], "savedAt": 1]]
        let d = try savedAccountsData(list)
        let text = String(decoding: d, as: UTF8.self)
        #expect(text.contains("\n"))
        #expect(text.contains("a/b@example.com"))
        #expect(savedAccounts(savedAccountEntries(d)).map(\.id) == ["A|org"])
        #expect((try JSONSerialization.jsonObject(with: d) as? [String: Any])?.keys.sorted() == ["accounts"])
    }
}

// MARK: - 전환할 때의 JSON 변환

@Suite("계정 전환: JSON 변환") struct AccountJSONTests {
    @Test("로그인 정보는 claudeAiOauth 만 바뀌고 mcpOAuth 같은 다른 키는 남는다") func 로그인_정보() {
        let cur: [String: Any] = ["claudeAiOauth": token("B"), "mcpOAuth": ["server": ["accessToken": "fake-mcp"]], "other": 1]
        let out = replacingAccountToken(cur, with: token("A"))
        #expect(tokenName(out) == "A")
        #expect(((out["mcpOAuth"] as? [String: Any])?["server"] as? [String: Any])?["accessToken"] as? String == "fake-mcp")
        #expect(out["other"] as? Int == 1)
        #expect(Set(out.keys) == ["claudeAiOauth", "mcpOAuth", "other"])
        let fresh = replacingAccountToken([:], with: token("A"))
        #expect(Set(fresh.keys) == ["claudeAiOauth"])
    }

    @Test("설정: 같은 계정이면 oauthAccount 만 바꾸고 캐시 키를 남긴다, 다르거나 앞 계정을 모르면 캐시 키를 모두 지운다", arguments: [
        ("A", false), ("B", true), (nil, true),
    ] as [(String?, Bool)])
    func 설정(_ previous: String?, cleared: Bool) {
        var j: [String: Any] = ["oauthAccount": oauthAccount("B"), "theme": "dark", "numStartups": 3]
        for k in accountCacheKeys { j[k] = ["x": 1] }
        switchConfig(&j, to: account("A"), previous: previous.map(id))
        #expect((j["oauthAccount"] as? [String: Any])?["accountUuid"] as? String == "A")
        #expect(j["theme"] as? String == "dark")
        #expect(j["numStartups"] as? Int == 3)
        #expect(accountCacheKeys.allSatisfy { j[$0] == nil } == cleared)
        #expect(accountCacheKeys.allSatisfy { j[$0] != nil } == !cleared)
    }

    @Test("계정별 캐시 키 목록은 Claude Code 2.1.300 기준 16개, 계정 정보 키는 들지 않는다") func 캐시_키() {
        #expect(accountCacheKeys.count == 16)
        #expect(Set(accountCacheKeys).count == 16)
        #expect(!accountCacheKeys.contains("oauthAccount"))
        #expect(accountCacheKeys.contains("cachedUsageUtilization"))
        #expect(accountCacheKeys.contains("startupPrefetchedAt"))
    }
}

// MARK: - 유스케이스: 저장·지우기

@Suite("계정 전환: 저장·지우기") struct AccountStoreTests {
    @Test("새 계정 — 보관함(계정 이름 = id)에 claudeAiOauth 만, 목록 끝에 더하고 저장 시각은 시계") func 새_계정() throws {
        let w = World()
        w.saved.data = data(["accounts": [["oauthAccount": oauthAccount("B"), "savedAt": 1]]])
        try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports)
        #expect(w.log.calls == ["write vault:A|org", "saved read", "saved write"])
        let stored = try #require(w.keychain.json(.savedAccount(id("A"))))
        #expect(Set(stored.keys) == ["claudeAiOauth"])
        #expect(tokenName(stored) == "A")
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("B"), id("A")])
        #expect(w.savedEntries.last?["savedAt"] as? Double == a0.timeIntervalSince1970)
        #expect(loadSavedAccounts(w.saved).last?.refreshExpires == Date(timeIntervalSince1970: 2_100_000_000))
    }

    @Test("같은 계정 다시 저장 — 제자리에서 바꾼다(차례 그대로)") func 다시_저장() throws {
        let w = World()
        w.saved.data = data(["accounts": [["oauthAccount": oauthAccount("A"), "savedAt": 1], ["oauthAccount": oauthAccount("B"), "savedAt": 2]]])
        try storeAccount(oauthAccount("A"), credentials: token("A2", refreshExpires: 2_200_000_000_000), ports: w.ports)
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("A"), id("B")])
        #expect(w.savedEntries.first?["savedAt"] as? Double == a0.timeIntervalSince1970)
        #expect(tokenName(w.keychain.json(.savedAccount(id("A")))) == "A2")
    }

    @Test("계정 정보에 accountUuid 가 없으면 아무것도 부르지 않고 \"계정 정보가 없습니다\"") func 계정_정보_없음() {
        let w = World()
        #expect(throws: AccountError("계정 정보가 없습니다")) {
            try storeAccount(["emailAddress": "x@example.com"], credentials: token("A"), ports: w.ports)
        }
        #expect(w.log.calls.isEmpty)
    }

    @Test("보관함에 못 쓰면 \"키체인에 저장하지 못했습니다\", 목록은 건드리지 않는다") func 보관함_쓰기_실패() {
        let w = World()
        w.keychain.failWrite = [.savedAccount(id("A"))]
        #expect(throws: AccountError("키체인에 저장하지 못했습니다")) {
            try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports)
        }
        #expect(w.log.calls == ["write vault:A|org"])
    }

    @Test("목록을 못 쓰면 그 오류를 그대로 던진다(보관함에는 이미 썼다)") func 목록_쓰기_실패() {
        let w = World()
        w.saved.writeError = TestError("목록을 쓰지 못함(시험)")
        #expect(throws: TestError.self) { try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports) }
        #expect(w.log.calls == ["write vault:A|org", "saved read", "saved write"])
        #expect(w.keychain.json(.savedAccount(id("A"))) != nil)
    }

    @Test("목록 파일이 깨졌으면 옆으로 한 번 치우고(시각은 시계) 새 항목 하나만 든 목록을 쓴다 — 던지지 않는다", arguments: brokenSavedFiles)
    func 깨진_목록(_ file: String) throws {
        let w = World()
        w.saved.data = Data(file.utf8)
        try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports)
        #expect(w.log.calls == ["write vault:A|org", "saved read", "saved set-aside \(a0.timeIntervalSince1970)", "saved write"])
        #expect(w.saved.aside == [Data(file.utf8)])
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("A")])
        #expect(w.savedEntries.count == 1)
    }

    @Test("목록 파일이 없거나 비었으면(공백·{}·accounts 없음) 치우지 않고 새 항목 하나로 쓴다", arguments: emptySavedFiles)
    func 빈_목록(_ file: String?) throws {
        let w = World()
        w.saved.data = file.map { Data($0.utf8) }
        try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports)
        #expect(w.log.calls == ["write vault:A|org", "saved read", "saved write"])
        #expect(w.saved.aside.isEmpty)
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("A")])
    }

    @Test("깨진 목록을 치우지 못하면 안내로 던지고 목록은 쓰지 않는다(보관함에는 이미 썼다)") func 깨진_목록_치우기_실패() {
        let w = World()
        let broken = Data("not json".utf8)
        w.saved.data = broken
        w.saved.setAsideError = TestError("옮기지 못함(시험)")
        #expect(throws: AccountError(setAsideFailed)) {
            try storeAccount(oauthAccount("A"), credentials: token("A"), ports: w.ports)
        }
        #expect(w.log.calls == ["write vault:A|org", "saved read", "saved set-aside \(a0.timeIntervalSince1970)"])
        #expect(w.saved.data == broken)
        #expect(w.keychain.json(.savedAccount(id("A"))) != nil)
    }

    @Test("지금 계정 저장 — 지금 계정이 대상 id 면 하지 않는다(건너뛸 id 가 없으면 저장)", arguments: [
        ("A", "B", true), ("A", "A", false), ("A", nil, true),
    ] as [(String?, String?, Bool)])
    func 지금_계정_저장(_ current: String?, skip: String?, stored: Bool) throws {
        let w = World()
        w.config.json = current.map { ["oauthAccount": oauthAccount($0)] } ?? [:]
        w.keychain.items[loginItem] = data(["claudeAiOauth": token("cur")])
        try saveCurrentAccount(except: skip.map(id), ports: w.ports)
        #expect((w.keychain.json(.savedAccount(id("A"))) != nil) == stored)
        if stored { #expect(tokenName(w.keychain.json(.savedAccount(id("A")))) == "cur") }
    }

    @Test("지금 계정 저장 — 계정 정보에 accountUuid 가 없는데 로그인 토큰이 있으면 까닭과 할 일을 담은 안내로 던진다(아무것도 쓰지 않는다)")
    func 지금_계정_정보_모자람() {
        let w = World()
        w.config.json = ["oauthAccount": ["emailAddress": "nouuid@example.com"]]
        w.keychain.items[loginItem] = data(["claudeAiOauth": token("cur")])
        #expect(throws: AccountError(currentIncomplete)) { try saveCurrentAccount(except: id("A"), ports: w.ports) }
        #expect(w.log.calls == ["config read", "read login"])
        #expect(w.keychain.items.keys.allSatisfy { $0 == loginItem })
    }

    @Test("지금 계정 저장 — 계정 정보에 accountUuid 가 없어도 로그인 토큰이 없으면 잃을 것이 없어 던지지 않는다") func 지금_계정_정보_모자람_토큰_없음() throws {
        let w = World()
        w.config.json = ["oauthAccount": ["emailAddress": "nouuid@example.com"]]
        w.keychain.items[loginItem] = data(["mcpOAuth": ["x": 1]])
        try saveCurrentAccount(except: id("A"), ports: w.ports)
        #expect(w.log.calls == ["config read", "read login"])
    }

    @Test("지금 계정 저장 — 계정 정보(oauthAccount)가 아예 없는데(설정 파일 없음 포함) 로그인 토큰이 있으면 안내로 던진다(아무것도 쓰지 않는다)",
          arguments: [true, false])
    func 지금_계정_정보_없음(_ fileExists: Bool) {
        let w = World()
        w.config.json = fileExists ? ["theme": "dark"] : nil
        w.keychain.items[loginItem] = data(["claudeAiOauth": token("cur")])
        #expect(throws: AccountError(currentMissing)) { try saveCurrentAccount(except: id("A"), incoming: token("A"), ports: w.ports) }
        #expect(w.log.calls == ["config read", "read login"])
        #expect(w.keychain.items.keys.allSatisfy { $0 == loginItem })
        #expect(w.saved.data == nil)
    }

    @Test("지금 계정 저장 — 계정 정보가 없어도 로그인 토큰이 없으면(항목 없음·claudeAiOauth 없음) 잃을 것이 없어 던지지 않는다",
          arguments: [true, false])
    func 지금_계정_정보_없음_토큰_없음(_ hasItem: Bool) throws {
        let w = World()
        w.config.json = [:]
        if hasItem { w.keychain.items[loginItem] = data(["mcpOAuth": ["x": 1]]) }
        try saveCurrentAccount(except: id("A"), incoming: token("A"), ports: w.ports)
        #expect(w.log.calls == ["config read", "read login"])
    }

    @Test("지금 계정 저장 — 계정 정보가 없거나 모자라도 로그인 토큰이 전환해 넣을 토큰과 같은 로그인(refreshToken 같음)이면 잃을 것이 없어 하지 않는다",
          arguments: [false, true])
    func 지금_계정_정보_없음_같은_로그인(_ noUuid: Bool) throws {
        let w = World()
        w.config.json = noUuid ? ["oauthAccount": ["emailAddress": "nouuid@example.com"]] : [:]
        var login = token("A")
        login["accessToken"] = "fake-access-A-refreshed"   // 액세스 토큰만 새로 받은 것도 같은 로그인
        w.keychain.items[loginItem] = data(["claudeAiOauth": login])
        try saveCurrentAccount(except: id("A"), incoming: token("A"), ports: w.ports)
        #expect(w.log.calls == ["config read", "read login"])
        #expect(w.keychain.items.keys.allSatisfy { $0 == loginItem })
        #expect(w.saved.data == nil)
    }

    @Test("지금 계정 저장 — refreshToken 이 다르거나 어느 한쪽에 없거나 비었으면(둘 다 없어도) 같은 로그인으로 보지 않고 던진다",
          arguments: ["다름", "넣을 쪽 없음", "지금 쪽 없음", "둘 다 없음", "둘 다 빈 글자", "글자 아님", "넣을 토큰 모름"])
    func 지금_계정_정보_없음_다른_로그인(_ kind: String) {
        let w = World()
        w.config.json = [:]
        var login = token("cur"), incoming: [String: Any]? = token("A")
        switch kind {
        case "넣을 쪽 없음": login["refreshToken"] = "fake-refresh-A"; incoming?["refreshToken"] = nil
        case "지금 쪽 없음": login["refreshToken"] = nil
        case "둘 다 없음": login["refreshToken"] = nil; incoming?["refreshToken"] = nil
        case "둘 다 빈 글자": login["refreshToken"] = ""; incoming?["refreshToken"] = ""
        case "글자 아님": login["refreshToken"] = 7; incoming?["refreshToken"] = 7
        case "넣을 토큰 모름": login = token("A"); incoming = nil   // 계정 추가 길(넣을 토큰 없이 부름)
        default: break
        }
        w.keychain.items[loginItem] = data(["claudeAiOauth": login])
        #expect(throws: AccountError(currentMissing)) { try saveCurrentAccount(except: id("A"), incoming: incoming, ports: w.ports) }
        #expect(w.keychain.items.keys.allSatisfy { $0 == loginItem })
    }

    @Test("지금 계정 저장 — 로그인 정보에 claudeAiOauth 가 없으면 하지 않는다") func 지금_계정_토큰_없음() throws {
        let w = World()
        w.config.json = ["oauthAccount": oauthAccount("A")]
        w.keychain.items[loginItem] = data(["mcpOAuth": ["x": 1]])
        try saveCurrentAccount(except: nil, ports: w.ports)
        #expect(w.log.calls == ["config read", "read login"])
    }

    @Test("지우기 — 보관함 항목을 지우고 지워졌는지 본 뒤 목록에서 뺀다(다른 항목·id 없는 항목은 남는다), 안내 없음") func 지우기() throws {
        let w = World()
        w.keychain.items[.savedAccount(id("A"))] = data(["claudeAiOauth": token("A")])
        w.keychain.items[.savedAccount(id("B"))] = data(["claudeAiOauth": token("B")])
        w.saved.data = data(["accounts": [["oauthAccount": oauthAccount("A")], ["savedAt": 1], ["oauthAccount": oauthAccount("B")]]])
        #expect(try removeAccount(id("A"), ports: w.ports) == nil)
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org", "saved read", "saved write"])
        #expect(w.keychain.items[.savedAccount(id("A"))] == nil)
        #expect(w.keychain.items[.savedAccount(id("B"))] != nil)
        #expect(w.savedEntries.count == 2)
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("B")])
    }

    @Test("지우기 — 키체인 항목이 원래 없었으면 실패로 보지 않고 목록에서 뺀다") func 지우기_항목_없음() throws {
        let w = World()
        w.saved.data = data(["accounts": [["oauthAccount": oauthAccount("A")], ["oauthAccount": oauthAccount("B")]]])
        #expect(try removeAccount(id("A"), ports: w.ports) == nil)
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org", "saved read", "saved write"])
        #expect(w.savedEntries.compactMap(savedAccountID) == [id("B")])
    }

    @Test("지우기 — 키체인 항목을 지우지 못하면(지운 뒤에도 남음) 안내로 던지고 목록은 건드리지 않는다(다시 지울 수 있게)") func 지우기_키체인_실패() {
        let w = World()
        let item = KeychainItem.savedAccount(id("A"))
        w.keychain.items[item] = data(["claudeAiOauth": token("A")])
        w.keychain.failDelete = [item]
        let list = data(["accounts": [["oauthAccount": oauthAccount("A")]]])
        w.saved.data = list
        #expect(throws: AccountError(removeKeychainFailed)) { try removeAccount(id("A"), ports: w.ports) }
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org"])
        #expect(w.keychain.items[item] != nil)
        #expect(w.saved.data == list)
    }

    @Test("지우기 — 목록을 못 쓰면 토큰은 지웠다는 것과 그 오류를 안내로 돌려준다(던지지 않는다)") func 지우기_실패() throws {
        let w = World()
        w.keychain.items[.savedAccount(id("A"))] = data(["claudeAiOauth": token("A")])
        w.saved.writeError = TestError("목록을 쓰지 못함(시험)")
        #expect(try removeAccount(id("A"), ports: w.ports) == removeListWriteFailed + "목록을 쓰지 못함(시험)")
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org", "saved read", "saved write"])
        #expect(w.keychain.items[.savedAccount(id("A"))] == nil)
    }

    @Test("지우기 — 목록 파일이 깨졌으면 키체인 항목만 지우고 목록 파일은 건드리지 않으며(치우지도 쓰지도 않는다) 그 안내를 돌려준다", arguments: brokenSavedFiles)
    func 지우기_깨진_목록(_ file: String) throws {
        let w = World()
        w.keychain.items[.savedAccount(id("A"))] = data(["claudeAiOauth": token("A")])
        w.saved.data = Data(file.utf8)
        #expect(try removeAccount(id("A"), ports: w.ports) == removeListBroken)
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org", "saved read"])
        #expect(w.keychain.items[.savedAccount(id("A"))] == nil)
        #expect(w.saved.data == Data(file.utf8))
    }

    @Test("지우기 — 목록 파일이 없거나 비었으면 빈 목록으로 쓴다, 안내 없음", arguments: emptySavedFiles)
    func 지우기_빈_목록(_ file: String?) throws {
        let w = World()
        w.saved.data = file.map { Data($0.utf8) }
        #expect(try removeAccount(id("A"), ports: w.ports) == nil)
        #expect(w.log.calls == ["delete vault:A|org", "exists vault:A|org", "saved read", "saved write"])
        #expect(w.saved.data != nil)
        #expect(writableSavedAccountEntries(w.saved.data)?.count == 0)
    }

    @Test("지우기 안내 글자에는 긴 줄표가 없다") func 지우기_안내_글자() {
        for m in [removeKeychainFailed, removeListBroken, removeListWriteFailed, currentIncomplete, currentMissing] { #expect(!m.contains("—")) }
    }
}

// MARK: - 유스케이스: 전환

enum LoginState: Sendable {
    case none          // 로그인 정보 항목 없음
    case unreadable    // 항목은 있는데 읽지 못함
    case garbage       // JSON 아님
    case array         // JSON 이지만 객체가 아님
    case full          // claudeAiOauth(B) + mcpOAuth
    case noToken       // mcpOAuth 만
    case target        // claudeAiOauth 가 대상 A 의 저장 토큰과 같은 로그인(refreshToken 같음) + mcpOAuth
}

enum ConfigState: Sendable {
    case missing       // ~/.claude.json 없음
    case noAccount     // oauthAccount 없음(로그인 전)
    case account(String)
    case noUuid        // oauthAccount 에 accountUuid 없음
}

struct SwitchRow: Row {
    let note: String
    var vault: String? = "token"       // 대상 A 의 보관함 내용: "token" = {"claudeAiOauth": A}, nil = 항목 없음, 그 밖은 그 글자 그대로
    var login = LoginState.full
    var config = ConfigState.account("B")
    var failWrite: [String] = []        // "login" · "vault:B"
    var savedWriteFails = false
    var savedBroken = false             // 저장 계정 목록 파일이 깨졌다
    var setAsideFails = false           // 깨진 목록을 옆으로 치우지 못한다
    var patchFails = false
    var lockFails = false
    let calls: [String]
    var error: String? = nil            // 던진 오류 글자
    var warning: String? = nil          // 돌려준 안내
    var switched = true                 // 로그인 정보의 claudeAiOauth 가 A 로 바뀌었나
    var savedB = false                  // 지금 계정 B 의 토큰을 보관함에 저장했나
    var listB: Bool? = nil              // 목록에도 B 가 들었나, nil 이면 savedB 와 같다
    var cachesCleared: Bool? = nil      // 설정을 고쳤으면 캐시 키를 지웠나, nil 이면 설정 그대로
    var side = SwitchFailureSide.other  // 던졌으면 그 오류의 쪽(자동 전환이 가른다) — 대상 계정의 저장 토큰이 없을 때만 대상 쪽
}

private let normal = ["read vault:A|org", "config read", "lock", "read login", "config read", "read login",
                      "write vault:B|org", "saved read", "saved write", "write login", "unlock", "config patch"]
private let noMatch = "저장된 로그인 정보가 없습니다. 계정을 다시 추가해 주세요"
private let unreadableLogin = "지금 로그인 정보를 읽지 못했습니다"

private let switchRows: [SwitchRow] = [
    .init(note: "B 에서 A 로 — 잠금 안에서 지금 계정(B) 토큰을 저장하고 claudeAiOauth 만 바꿔 쓴 뒤, 잠금 밖에서 설정을 고치고 캐시를 지운다",
          calls: normal, savedB: true, cachesCleared: true),
    .init(note: "대상이 지금 계정(A 에서 A) — 지금 계정 저장을 건너뛰고 캐시는 남긴다", config: .account("A"),
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "write login", "unlock", "config patch"],
          cachesCleared: false),
    .init(note: "저장 토큰 없음 → 아무것도 바꾸지 않는다", vault: nil, calls: ["read vault:A|org"], error: noMatch, switched: false, side: .target),
    .init(note: "저장 토큰에 claudeAiOauth 없음 → 저장 토큰 없음", vault: #"{"other":1}"#, calls: ["read vault:A|org"], error: noMatch, switched: false, side: .target),
    .init(note: "저장 토큰이 JSON 아님 → 저장 토큰 없음", vault: "garbage", calls: ["read vault:A|org"], error: noMatch, switched: false, side: .target),
    .init(note: "로그인 정보 항목은 있는데 못 읽음 → 잠금 안에서 멈춘다", login: .unreadable,
          calls: ["read vault:A|org", "config read", "lock", "read login", "exists login", "unlock"], error: unreadableLogin, switched: false),
    .init(note: "로그인 정보가 JSON 아님 → 있는지 묻지 않고 멈춘다", login: .garbage,
          calls: ["read vault:A|org", "config read", "lock", "read login", "unlock"], error: unreadableLogin, switched: false),
    .init(note: "로그인 정보가 JSON 배열 → 멈춘다", login: .array,
          calls: ["read vault:A|org", "config read", "lock", "read login", "unlock"], error: unreadableLogin, switched: false),
    .init(note: "로그인 정보 항목 없음 → 빈 것에서 시작(지금 계정 토큰이 없어 저장도 건너뜀)", login: .none,
          calls: ["read vault:A|org", "config read", "lock", "read login", "exists login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "로그인 정보에 claudeAiOauth 없음(mcpOAuth 만) → 지금 계정 저장은 건너뛰고 mcpOAuth 는 남긴다", login: .noToken,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "설정에 계정 정보가 없는데 로그인 토큰(B)이 있으면 그 계정을 저장하지 못해 전환을 막고(토큰을 잃지 않게) 까닭과 할 일을 안내한다",
          config: .noAccount,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "unlock"],
          error: currentMissing, switched: false),
    .init(note: "설정 파일이 없는데 로그인 토큰(B)이 있으면 같은 까닭으로 막는다", config: .missing,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "unlock"],
          error: currentMissing, switched: false),
    .init(note: "설정에 계정 정보 없음 + 로그인 토큰 없음(mcpOAuth 만) → 잃을 것이 없어 전환, 앞 계정을 몰라 캐시를 지운다",
          login: .noToken, config: .noAccount,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "설정에 계정 정보 없음 + 로그인 정보 항목 없음 → 전환", login: .none, config: .noAccount,
          calls: ["read vault:A|org", "config read", "lock", "read login", "exists login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "설정에 계정 정보 없음 + 로그인 토큰이 대상 A 의 저장 토큰과 같은 로그인 → 잃을 것이 없어 전환", login: .target, config: .noAccount,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "설정 파일 없음 + 로그인 토큰 없음 → 토큰은 바꾸고 설정 고치기 실패 안내", login: .noToken, config: .missing,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          warning: "토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다: .claude.json 를 읽지 못했습니다"),
    .init(note: "설정 고치기 실패 → 토큰은 바꾸고 안내를 돌려준다(던지지 않는다)", patchFails: true, calls: normal,
          warning: "토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다: 설정을 쓰지 못함(시험)", savedB: true),
    .init(note: "로그인 정보 쓰기 실패 → \"키체인에 쓰지 못했습니다\", 설정은 고치지 않는다(지금 계정 저장은 이미 했다)", failWrite: ["login"],
          calls: Array(normal.dropLast()), error: "키체인에 쓰지 못했습니다", switched: false, savedB: true),
    .init(note: "지금 계정 토큰을 보관함에 못 씀 → \"키체인에 저장하지 못했습니다\", 로그인 정보는 그대로", failWrite: ["vault:B"],
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write vault:B|org", "unlock"],
          error: "키체인에 저장하지 못했습니다", switched: false),
    .init(note: "저장 계정 목록을 못 씀 → 그 오류로 멈추고 로그인 정보는 그대로", savedWriteFails: true,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write vault:B|org", "saved read", "saved write", "unlock"],
          error: "목록을 쓰지 못함(시험)", switched: false, savedB: true, listB: false),
    .init(note: "저장 계정 목록이 깨졌으면 옆으로 치우고 지금 계정(B)만 든 새 목록을 써서 전환을 마친다", savedBroken: true,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write vault:B|org", "saved read",
                  "saved set-aside \(a0.timeIntervalSince1970)", "saved write", "write login", "unlock", "config patch"],
          savedB: true, cachesCleared: true),
    .init(note: "깨진 목록을 치우지 못하면 그 안내로 멈추고 로그인 정보는 그대로", savedBroken: true, setAsideFails: true,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write vault:B|org", "saved read",
                  "saved set-aside \(a0.timeIntervalSince1970)", "unlock"],
          error: setAsideFailed, switched: false, savedB: true, listB: false),
    .init(note: "쓰기 잠금을 못 잡음 → 잠금 안의 일은 하나도 하지 않는다", lockFails: true,
          calls: ["read vault:A|org", "config read", "lock failed"], error: "Claude Code 가 로그인 정보를 쓰는 중입니다(시험)", switched: false),
    .init(note: "설정의 계정 정보에 accountUuid 가 없으면 지금 계정을 저장하지 못해 전환을 막고(토큰을 잃지 않게), 까닭과 할 일을 안내한다",
          config: .noUuid,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "unlock"],
          error: currentIncomplete, switched: false),
    .init(note: "설정의 계정 정보에 accountUuid 가 없어도 로그인 토큰이 없으면(mcpOAuth 만) 잃을 것이 없어 전환한다", login: .noToken,
          config: .noUuid,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
    .init(note: "설정의 계정 정보에 accountUuid 가 없어도 로그인 토큰이 대상 A 와 같은 로그인이면 잃을 것이 없어 전환한다", login: .target,
          config: .noUuid,
          calls: ["read vault:A|org", "config read", "lock", "read login", "config read", "read login", "write login", "unlock", "config patch"],
          cachesCleared: true),
]

@Suite("계정 전환: 전환 유스케이스") struct AccountSwitchTests {
    @Test("저장 토큰 → 잠금 안(지금 로그인 정보 읽기 → 지금 계정 저장 → claudeAiOauth 만 바꿔 쓰기) → 잠금 밖 설정 고치기", arguments: switchRows)
    func 표(_ row: SwitchRow) throws {
        let w = World()
        let vaultA = KeychainItem.savedAccount(id("A"))
        switch row.vault {
        case nil: break
        case "token"?: w.keychain.items[vaultA] = data(["claudeAiOauth": token("A")])
        case let raw?: w.keychain.items[vaultA] = Data(raw.utf8)
        }
        let mcp: [String: Any] = ["server": ["accessToken": "fake-mcp"]]
        switch row.login {
        case .none: break
        case .unreadable: w.keychain.unreadable = [loginItem]
        case .garbage: w.keychain.items[loginItem] = Data("garbage".utf8)
        case .array: w.keychain.items[loginItem] = Data("[]".utf8)
        case .full: w.keychain.items[loginItem] = data(["claudeAiOauth": token("B"), "mcpOAuth": mcp])
        case .noToken: w.keychain.items[loginItem] = data(["mcpOAuth": mcp])
        case .target:
            var t = token("A")
            t["accessToken"] = "fake-access-A-refreshed"   // 액세스 토큰만 새로 받았어도 refreshToken 이 같으면 같은 로그인
            w.keychain.items[loginItem] = data(["claudeAiOauth": t, "mcpOAuth": mcp])
        }
        var config: [String: Any] = ["theme": "dark"]
        for k in accountCacheKeys { config[k] = 1 }
        switch row.config {
        case .missing: w.config.json = nil
        case .noAccount: w.config.json = config
        case .account(let u): config["oauthAccount"] = oauthAccount(u); w.config.json = config
        case .noUuid: config["oauthAccount"] = ["emailAddress": "nouuid@example.com"]; w.config.json = config
        }
        let configBefore = w.config.json.map { data($0) }
        w.keychain.failWrite = Set(row.failWrite.map { $0 == "login" ? loginItem : .savedAccount(id(String($0.dropFirst(6)))) })
        if row.savedWriteFails { w.saved.writeError = TestError("목록을 쓰지 못함(시험)") }
        if row.savedBroken { w.saved.data = Data(#"{"accounts":[{"oauthAccount":{"accountUuid":"C","organizationUuid":"org"}},"broken"]}"#.utf8) }
        if row.setAsideFails { w.saved.setAsideError = TestError("옮기지 못함(시험)") }
        if row.patchFails { w.config.patchError = TestError("설정을 쓰지 못함(시험)") }
        if row.lockFails { w.lock.error = AccountError("Claude Code 가 로그인 정보를 쓰는 중입니다(시험)") }
        let loginBefore = w.keychain.items[loginItem]

        var warning: String?
        var thrown: String?
        var side: SwitchFailureSide?
        do { warning = try switchAccount(to: account("A"), ports: w.ports) } catch {
            thrown = error.localizedDescription
            side = switchFailureSide(error)
        }

        #expect(thrown == row.error)
        #expect(side == (row.error == nil ? nil : row.side))
        #expect(warning == row.warning)
        #expect(w.log.calls == row.calls)
        // 로그인 정보: 바뀌었으면 claudeAiOauth 만 A 이고 다른 키(mcpOAuth)는 그대로
        if row.switched {
            let after = try #require(w.keychain.json(loginItem))
            #expect(tokenName(after) == "A")
            let hadMcp = row.login == .full || row.login == .noToken || row.login == .target
            #expect(Set(after.keys) == (hadMcp ? ["claudeAiOauth", "mcpOAuth"] : ["claudeAiOauth"]))
            if hadMcp { #expect(((after["mcpOAuth"] as? [String: Any])?["server"] as? [String: Any])?["accessToken"] as? String == "fake-mcp") }
        } else {
            #expect(w.keychain.items[loginItem] == loginBefore)
        }
        // 지금 계정 B 저장
        #expect((w.keychain.json(.savedAccount(id("B"))) != nil) == row.savedB)
        if row.savedB { #expect(tokenName(w.keychain.json(.savedAccount(id("B")))) == "B") }
        #expect(w.savedEntries.compactMap(savedAccountID) == ((row.listB ?? row.savedB) ? [id("B")] : []))
        #expect(w.keychain.items.keys.allSatisfy { $0 == loginItem || $0.service == KeychainItem.savedAccountService }, "다른 키체인 항목을 건드렸다")
        // 설정
        if let cleared = row.cachesCleared {
            let after = try #require(w.config.json)
            #expect((after["oauthAccount"] as? [String: Any])?["accountUuid"] as? String == "A")
            #expect(after["theme"] as? String == "dark")
            #expect(accountCacheKeys.allSatisfy { after[$0] == nil } == cleared)
            #expect(accountCacheKeys.allSatisfy { after[$0] != nil } == !cleared)
        } else {
            #expect(w.config.json.map { data($0) } == configBefore)
        }
    }

    @Test("전환 실패의 쪽 — AccountError 가 담은 쪽(기본은 그 밖), AccountError 가 아닌 오류(목록 파일 쓰기 등)는 그 밖") func 실패_쪽() {
        #expect(switchFailureSide(AccountError("x")) == .other)
        #expect(switchFailureSide(AccountError("x", side: .target)) == .target)
        #expect(switchFailureSide(AccountError("x", side: .other)) == .other)
        #expect(switchFailureSide(TestError("목록을 쓰지 못함(시험)")) == .other)
        #expect(switchFailureSide(CocoaError(.fileWriteNoPermission)) == .other)
    }

    @Test("지금 계정 읽기 — 설정의 oauthAccount 로 만든 계정, 없거나 accountUuid 가 없으면 nil") func 지금_계정() {
        let w = World()
        #expect(currentAccount(w.config) == nil)
        w.config.json = ["oauthAccount": oauthAccount("A")]
        #expect(currentAccount(w.config)?.id == id("A"))
        w.config.json = ["oauthAccount": ["emailAddress": "x@example.com"]]
        #expect(currentAccount(w.config) == nil)
    }
}
