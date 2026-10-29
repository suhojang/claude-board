// 계정 전환 규칙 — 저장 계정 모델, 저장 계정 목록(accounts.json) 해석·고치기, 전환할 때의 JSON 변환, 저장·전환·지우기 유스케이스
// (AccountStore 에 있던 것을 옮김, 동작·호출 순서·오류 글자는 그대로, 옮긴 뒤 고친 것: 깨진 accounts.json 은 쓰기 전에 옆으로 치운다,
// 이슈 #9 3차: 요금제 글자의 앞 빈칸, 지우기의 결과 안내, 지금 계정 정보가 모자라 전환을 막을 때의 안내,
// 지금 계정 정보가 아예 없어도 로그인 토큰이 있으면 전환을 막음, 전환 실패의 쪽(자동 전환이 가른다)).
// 바깥(키체인·~/.claude.json·accounts.json·쓰기 잠금·시계)은 포트(Ports.swift)로만 부른다.
//
// 계정 전환: 계정마다 브라우저 로그인을 한 번 받아 토큰을 저장해 두고, 고르면 Claude Code 로그인 항목의 토큰만 바꿔 넣는다.
// 실행 중인 세션은 요청을 보낼 때마다 키체인을 다시 읽으므로(최대 30초 캐시) 모든 세션이 다음 요청부터 새 계정으로 이어 간다.
// 상태(@Published current·saved·switching, 큐, 3초 감시)와 계정 추가 로그인 흐름은 앱(AccountStore)이 쥔다
import Foundation

// MARK: - 저장 계정 모델

/// 저장해 둔 Claude 계정 — 토큰은 키체인(KeychainItem.savedAccount(id))에만 두고 여기에는 화면에 보일 정보만 담는다.
/// oauth 가 [String: Any](JSONSerialization 결과, ~/.claude.json 에 그대로 다시 넣는다)라 Sendable 이 아니다 —
/// 앱은 메인 스레드와 AccountStore 의 큐에서만 쓴다
public struct Account: Identifiable, Equatable {
    public let id: String                  // accountUuid|organizationUuid — 같은 사람도 조직이 다르면 토큰이 따로다
    public let oauth: [String: Any]        // ~/.claude.json 의 oauthAccount 그대로 — 전환할 때 이 값으로 바꿔 넣는다
    public var refreshExpires: Date?       // 이 시각이 지나면 다시 로그인해야 한다

    /// accountUuid 가 없거나 빈 글자면 nil. organizationUuid 가 없으면 빈 글자로 붙인다
    public init?(oauth: [String: Any], refreshExpires: Date? = nil) {
        guard let uuid = oauth["accountUuid"] as? String, !uuid.isEmpty else { return nil }
        id = uuid + "|" + (oauth["organizationUuid"] as? String ?? "")
        self.oauth = oauth
        self.refreshExpires = refreshExpires
    }

    public var email: String { oauth["emailAddress"] as? String ?? "?" }
    public var org: String { oauth["organizationName"] as? String ?? "" }
    /// 요금제 — organizationType claude_max + 한도 등급 …_5x → "Max 5x". 비지 않은 조각만 빈칸으로 잇는다(등급만 있으면 "5x")
    public var plan: String {
        let t = (oauth["organizationType"] as? String ?? "").replacingOccurrences(of: "claude_", with: "")
        var parts = [t.prefix(1).uppercased() + t.dropFirst()]
        if let tier = oauth["organizationRateLimitTier"] as? String,
           let r = tier.range(of: "_\\d+x$", options: .regularExpression) { parts.append(String(tier[r].dropFirst())) }
        return parts.filter { !$0.isEmpty }.joined(separator: " ")
    }
    /// 다시 로그인해야 하나 — refreshExpires 가 now 보다 앞일 때만(같은 시각은 아직 아니다). 없으면 만료되지 않는다
    public func isExpired(at now: Date) -> Bool { refreshExpires.map { $0 < now } ?? false }

    // 화면에 보이는 값만 견준다 — oauth 의 다른 필드가 바뀌어도 같은 계정이다
    public static func == (a: Account, b: Account) -> Bool {
        a.id == b.id && a.email == b.email && a.org == b.org && a.plan == b.plan && a.refreshExpires == b.refreshExpires
    }
}

/// 계정 전환·저장의 오류 — 글자가 그대로 안내(flash)에 나온다
/// 계정 전환 실패의 쪽 — 자동 전환이 다음에 할 일을 가른다(AutoSwitch.swift 의 autoSwitchFailed)
public enum SwitchFailureSide: Equatable, Sendable {
    /// 대상 계정 쪽 — 그 계정의 저장된 로그인 정보가 없거나 읽지 못함. 그 계정만 한동안 고르지 않으면 된다
    case target
    /// 그 밖 — 지금 계정(저장할 수 없음·지금 로그인 정보를 읽지 못함)이나 시스템(키체인·목록을 쓰지 못함) 쪽. 다른 계정으로 해도 같다
    case other
    /// 쓰기 잠금을 잡지 못함 — Claude Code 가 로그인 정보를 쓰는 중이라 곧 풀린다. 잠깐만 쉬고 같은 판단을 다시 한다
    /// (그 밖처럼 1시간 쉬면 한도로 멈춘 세션이 그만큼 기다린다)
    case busy
}

/// 계정 저장·전환·지우기의 오류 — 글자가 그대로 안내(flash)에 나온다. side 는 전환 실패의 쪽(기본은 그 밖)
public struct AccountError: LocalizedError, Equatable {
    public let errorDescription: String?
    public let side: SwitchFailureSide
    public init(_ m: String, side: SwitchFailureSide = .other) { errorDescription = m; self.side = side }
}

/// 전환이 던진 오류의 쪽 — AccountError 는 담은 쪽, 그 밖의 오류(목록 파일 쓰기 등 포트가 던진 것)는 그 밖
public func switchFailureSide(_ error: Error) -> SwitchFailureSide { (error as? AccountError)?.side ?? .other }

// MARK: - 저장 계정 목록 (accounts.json)
// {"accounts": [{"oauthAccount": {…}, "savedAt": 초, "refreshTokenExpiresAt": 밀리초}, …]}

/// 저장 계정 목록 파일의 항목들(표시용). 없거나 JSON 객체가 아니거나 accounts 가 객체 배열이 아니면 빈 목록 — 쓰기 길은 writableSavedAccountEntries
public func savedAccountEntries(_ data: Data?) -> [[String: Any]] {
    guard let data, let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return [] }
    return j["accounts"] as? [[String: Any]] ?? []
}

/// 쓰기 길(저장·지우기)에서 읽는 항목들 — 표시용 savedAccountEntries 는 깨진 파일도 빈 목록으로 읽어, 그 빈 목록에 더해 쓰면
/// 다른 저장 계정의 표시 정보를 덮어쓴다. 그래서 쓰기 전에는 깨짐을 가른다:
/// 없음·빈 바이트·JSON 공백만·accounts 키 없음({} 포함) → 빈 목록, 객체 배열 → 그대로,
/// 그 밖(JSON 아님·최상위가 객체 아님·accounts 가 배열 아님(null 포함)·객체 아닌 항목이 섞임) → nil(깨짐). 섞인 것도 일부 살리지 않고 깨짐으로 본다
public func writableSavedAccountEntries(_ data: Data?) -> [[String: Any]]? {
    guard let data, !data.allSatisfy({ [0x20, 0x09, 0x0A, 0x0D].contains($0) }) else { return [] }
    guard let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
    guard let raw = j["accounts"] else { return [] }
    guard let items = raw as? [Any] else { return nil }
    let list = items.compactMap { $0 as? [String: Any] }
    return list.count == items.count ? list : nil
}

/// 항목의 계정 id — oauthAccount 가 없거나 accountUuid 가 없으면 nil
public func savedAccountID(_ entry: [String: Any]) -> String? {
    (entry["oauthAccount"] as? [String: Any]).flatMap { Account(oauth: $0) }?.id
}

/// 항목들 → 계정(파일 차례). oauthAccount 가 없거나 계정 id 를 못 만드는 항목은 뺀다. refreshTokenExpiresAt 은 밀리초(숫자가 아니면 없음)
public func savedAccounts(_ entries: [[String: Any]]) -> [Account] {
    entries.compactMap { e -> Account? in
        guard let oa = e["oauthAccount"] as? [String: Any] else { return nil }
        let exp = (e["refreshTokenExpiresAt"] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue / 1000) }
        return Account(oauth: oa, refreshExpires: exp)
    }
}

/// 저장할 항목 — 보일 정보(oauthAccount)·저장 시각(초)·토큰의 refreshTokenExpiresAt(있으면 그대로)
public func savedAccountEntry(oauthAccount oa: [String: Any], credentials oauth: [String: Any], savedAt: Date) -> [String: Any] {
    var e: [String: Any] = ["oauthAccount": oa, "savedAt": savedAt.timeIntervalSince1970]
    if let r = oauth["refreshTokenExpiresAt"] { e["refreshTokenExpiresAt"] = r }
    return e
}

/// 같은 계정 id 항목은 제자리에서 바꾸고(처음 것), 없으면 끝에 더한다
public func upsertingSavedAccount(_ list: [[String: Any]], _ entry: [String: Any], id: String) -> [[String: Any]] {
    var list = list
    if let i = list.firstIndex(where: { savedAccountID($0) == id }) { list[i] = entry } else { list.append(entry) }
    return list
}

/// 그 계정 id 항목을 모두 뺀다. id 를 못 만드는 항목은 남는다
public func removingSavedAccount(_ list: [[String: Any]], id: String) -> [[String: Any]] {
    list.filter { savedAccountID($0) != id }
}

/// 저장 계정 목록 파일 내용 — 사람이 읽기 쉽게 줄을 나누고 / 는 그대로
public func savedAccountsData(_ list: [[String: Any]]) throws -> Data {
    try JSONSerialization.data(withJSONObject: ["accounts": list], options: [.prettyPrinted, .withoutEscapingSlashes])
}

// MARK: - 전환할 때의 JSON 변환

/// Claude Code 가 /login 으로 다른 계정에 들어갈 때 ~/.claude.json 에서 비우는 계정별 캐시(2.1.300 기준) —
/// 같이 비워야 새 계정의 모델 목록·조직 기본 모델·사용량 표시를 다시 받는다
public let accountCacheKeys = ["additionalModelOptionsCache", "additionalModelOptionsAnsweredAt", "additionalModelCostsCache",
                               "modelAccessCache", "orgModelDefaultCache", "cachedArtifactRoster", "artifactRosterDenied",
                               "lastSeenOrgDefaultUpdatedAt", "clientDataCache", "clientDataCacheSlots", "autoCompactWindowsCache",
                               "cachedUsageUtilization", "metricsStatusCache", "metricsStatusCacheByPrincipal",
                               "githubWebConnectionStatusCache", "startupPrefetchedAt"]

/// 로그인 정보 JSON 에서 계정 토큰(claudeAiOauth)만 바꾼다 — MCP 서버 로그인(mcpOAuth) 같은 다른 값은 그대로 둔다
public func replacingAccountToken(_ credentials: [String: Any], with oauth: [String: Any]) -> [String: Any] {
    var cur = credentials
    cur["claudeAiOauth"] = oauth
    return cur
}

/// ~/.claude.json 을 새 계정으로 — oauthAccount 를 바꾸고(/status·claude auth status·보드 계정 표시), 앞 계정과 다르면(앞 계정을 모르면 포함)
/// 계정별 캐시를 지운다. 다른 키는 그대로
public func switchConfig(_ j: inout [String: Any], to target: Account, previous: String?) {
    j["oauthAccount"] = target.oauth
    if previous != target.id { for k in accountCacheKeys { j[k] = nil } }
}

// MARK: - 유스케이스

/// 계정 전환·저장·지우기가 부르는 바깥
public struct AccountPorts: Sendable {
    public let keychain: KeychainItems
    public let login: KeychainItem              // Claude Code 로그인 정보 항목(CLAUDE_CONFIG_DIR 없이 띄운 세션들이 쓰는 것)
    public let config: ClaudeConfig             // ~/.claude.json
    public let saved: SavedAccountList          // ~/.claude/notifier/accounts.json
    public let writeLock: CredentialsWriteLock  // ~/.claude/.storage-write
    public let clock: Clock
    public init(keychain: KeychainItems, login: KeychainItem, config: ClaudeConfig, saved: SavedAccountList,
                writeLock: CredentialsWriteLock, clock: Clock) {
        self.keychain = keychain; self.login = login; self.config = config; self.saved = saved
        self.writeLock = writeLock; self.clock = clock
    }
}

/// 지금 로그인된 계정 — ~/.claude.json 의 oauthAccount
public func currentAccount(_ config: ClaudeConfig) -> Account? {
    config.oauthAccount().flatMap { Account(oauth: $0) }
}

/// 저장해 둔 계정(파일 차례)
public func loadSavedAccounts(_ saved: SavedAccountList) -> [Account] {
    savedAccounts(savedAccountEntries(saved.read()))
}

/// 깨진 저장 계정 목록을 옆으로 치우지 못했을 때의 안내 — 글자가 그대로 안내(flash)에 나온다
let savedAccountsSetAsideFailed = "저장 계정 목록(accounts.json)을 읽지 못해 옆으로 옮기려 했지만 옮기지 못했습니다"
/// 지금 로그인된 계정의 정보(~/.claude.json 의 oauthAccount)에 accountUuid 가 없어 그 계정을 저장하지 못할 때 — 전환 실패 안내 뒤에 붙는다
let currentAccountIncomplete = "지금 로그인된 계정의 정보가 모자라 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요"
/// 지금 로그인된 계정의 정보(oauthAccount)가 아예 없어(~/.claude.json 이 없을 때 포함) 그 계정을 저장하지 못할 때 — 위와 끝이 같고 까닭만 다르다
let currentAccountMissing = "지금 로그인된 계정의 정보가 없어 그 계정을 먼저 저장할 수 없습니다(그대로 바꾸면 그 로그인을 잃습니다). Claude Code 에서 /login 으로 다시 로그인한 뒤 해 주세요"
/// 저장 계정 지우기의 안내 — 키체인에서 못 지움(던짐), 목록이 깨짐·목록을 못 씀(돌려줌, 앞에 오류 글자가 붙는다)
let removeKeychainFailed = "키체인의 로그인 정보를 지우지 못해 목록에 그대로 두었습니다. 잠시 뒤 다시 지워 주세요"
let removeListBroken = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 읽지 못해 목록은 그대로 두었습니다"
let removeListWriteFailed = "로그인 정보는 지웠지만 저장 계정 목록(accounts.json)을 쓰지 못했습니다: "

/// 계정 토큰(claudeAiOauth)은 키체인에, 보일 정보는 accounts.json 에 넣는다 — 같은 계정은 제자리에서 바꾼다.
/// 차례: 키체인 쓰기 → 목록 읽기 → (깨졌으면 옆으로 치우기) → 목록 쓰기. 키체인에 못 쓰면 목록은 건드리지 않는다.
/// 목록이 깨졌으면 던지지 않고 옆으로 치운 뒤 빈 목록에서 새 항목 하나로 쓴다 — switchAccount 가 새 토큰을 쓰기 전에 여기를 부르므로,
/// 던지면 파일이 깨진 동안 모든 계정 전환이 막힌다. 다른 계정의 토큰은 키체인에 남아 있어 치운 파일에서 항목을 되살리면 된다.
/// 치우지 못하면 그때만 안내로 던지고 쓰지 않는다
public func storeAccount(_ oa: [String: Any], credentials oauth: [String: Any], ports: AccountPorts) throws {
    guard let a = Account(oauth: oa) else { throw AccountError("계정 정보가 없습니다") }
    guard try ports.keychain.write(.savedAccount(a.id), json: ["claudeAiOauth": oauth]) else {
        throw AccountError("키체인에 저장하지 못했습니다")
    }
    let e = savedAccountEntry(oauthAccount: oa, credentials: oauth, savedAt: ports.clock.now)
    let entries: [[String: Any]]
    if let read = writableSavedAccountEntries(ports.saved.read()) {
        entries = read
    } else {
        do { try ports.saved.setAside(at: ports.clock.now) } catch { throw AccountError(savedAccountsSetAsideFailed) }
        entries = []
    }
    try ports.saved.write(savedAccountsData(upsertingSavedAccount(entries, e, id: a.id)))
}

/// 지금 로그인된 계정의 최신 토큰을 저장한다 — 쓰는 동안 갱신되며 바뀐 토큰이라, 처음 저장했던 것은 이미 못 쓸 수 있다.
/// 지금 계정이 skip 이거나 로그인 토큰이 없으면 하지 않는다. 쓰기 잠금 안에서 부른다.
/// 로그인 토큰은 있는데 그 계정을 저장할 수 없으면 — 계정 정보(oauthAccount)가 아예 없거나(currentAccountMissing) accountUuid 가 없어
/// 계정을 만들지 못하면(currentAccountIncomplete) — 던진다. 저장하지 못한 채 전환이 그 토큰을 덮어쓰면 그 계정을 잃으므로 전환을 막고,
/// 까닭과 할 일을 알린다. 다만 그 토큰이 incoming(전환이 로그인 항목에 넣을 대상 계정의 저장 토큰)과 같은 로그인이면 잃을 것이 없어
/// 막지 않는다(sameLogin). 계정 추가는 incoming 없이 try? 로 불러 던져도 저장만 건너뛴다.
/// storeAccount 의 "계정 정보가 없습니다" 와 따로 둔다(계정 추가는 계정을 만든 뒤 storeAccount 를 부른다)
public func saveCurrentAccount(except skip: String?, incoming: [String: Any]? = nil, ports: AccountPorts) throws {
    let oa = ports.config.oauthAccount()
    let cur = oa.flatMap { Account(oauth: $0) }
    if let cur, cur.id == skip { return }
    guard let co = credentialsJSON(ports.keychain.read(ports.login))?["claudeAiOauth"] as? [String: Any] else { return }
    guard let oa, cur != nil else {
        if let incoming, sameLogin(co, incoming) { return }
        throw AccountError(oa == nil ? currentAccountMissing : currentAccountIncomplete)
    }
    try storeAccount(oa, credentials: co, ports: ports)
}

/// 두 토큰 묶음(claudeAiOauth)이 같은 로그인인가 — refreshToken 이 둘 다 비지 않은 글자이고 같을 때만.
/// 토큰 묶음에는 계정 id 가 없고, accessToken 은 몇 시간마다 새로 받으며 refreshToken 도 갱신 때 바뀐다. 그래서 같은 계정인지는 알 수 없고,
/// refreshToken 이 같다는 것은 지금 로그인 토큰이 보드가 저장해 둔(그리고 전환으로 넣은) 그 로그인이라는 뜻이다 — 그 로그인은 저장 토큰에
/// 그대로 남으므로 덮어써도 잃지 않는다. 한쪽이라도 없거나 다르면 같지 않다고 본다(모르면 막는 쪽)
private func sameLogin(_ a: [String: Any], _ b: [String: Any]) -> Bool {
    guard let x = a["refreshToken"] as? String, !x.isEmpty, let y = b["refreshToken"] as? String else { return false }
    return x == y
}

/// 저장한 계정으로 바꾼다. 고칠 수 없었던 부분이 있으면 그 안내를 돌려준다(토큰은 바꿨다). 실패하면 던진다(바꾸지 않았다).
/// 던진 오류의 쪽(switchFailureSide)은 대상 계정의 저장 토큰이 없거나 못 읽을 때만 대상 쪽이고, 나머지는 모두 그 밖이다.
/// 차례: 저장한 토큰 읽기 → 앞 계정 id 읽기 → [쓰기 잠금: 지금 로그인 정보 읽기 → 지금 계정 토큰 저장(대상이면 건너뜀, 로그인 토큰이 있는데
/// 그 계정을 저장할 수 없으면 막음) → claudeAiOauth 만 바꿔 쓰기] → 잠금 밖에서 ~/.claude.json 고치기
public func switchAccount(to target: Account, ports: AccountPorts) throws -> String? {
    guard let oauth = credentialsJSON(ports.keychain.read(.savedAccount(target.id)))?["claudeAiOauth"] as? [String: Any] else {
        throw AccountError("저장된 로그인 정보가 없습니다. 계정을 다시 추가해 주세요", side: .target)
    }
    // CLAUDE_CONFIG_DIR 을 따로 정해 띄운 세션은 이름 뒤에 경로 해시가 붙은 다른 항목을 쓴다 — 그 항목은 건드리지 않는다(앱이 login 으로 주 항목만 넘긴다).
    // 같은 토큰을 두 항목에 넣으면 한쪽이 갱신(회전)할 때 다른 쪽 토큰이 무효가 되기 때문
    let previous = currentAccount(ports.config)?.id
    // Claude Code 가 토큰을 갱신해 쓸 때 잡는 잠금 — 그 사이에 끼어 앞 계정 토큰으로 덮이지 않게 한다
    try ports.writeLock.withLock {
        var cur: [String: Any] = [:]
        if let raw = ports.keychain.read(ports.login) {
            guard let j = credentialsJSON(raw) else { throw AccountError("지금 로그인 정보를 읽지 못했습니다") }
            cur = j
        } else if ports.keychain.exists(ports.login) {
            throw AccountError("지금 로그인 정보를 읽지 못했습니다")
        }
        try saveCurrentAccount(except: target.id, incoming: oauth, ports: ports)
        // 계정 토큰만 바꿔 넣는다 — MCP 서버 로그인(mcpOAuth) 같은 다른 값은 그대로 둔다
        guard try ports.keychain.write(ports.login, json: replacingAccountToken(cur, with: oauth)) else {
            throw AccountError("키체인에 쓰지 못했습니다")
        }
    }
    // 계정 정보 — /status·claude auth status·보드 계정 표시가 새 계정으로 보이게. 다른 계정이면 앞 계정의 캐시도 비운다
    do {
        try ports.config.patch { switchConfig(&$0, to: target, previous: previous) }
    } catch {
        return "토큰은 바꿨지만 계정 정보 파일을 고치지 못했습니다: \(error.localizedDescription)"
    }
    return nil
}

/// 저장한 계정을 지운다 — 키체인의 토큰을 지우고, 지워졌는지 본 뒤 목록에서 뺀다. 다 했으면 nil.
/// 키체인 항목이 지운 뒤에도 남아 있으면 던지고 목록은 건드리지 않는다(목록에서 빼면 다시 지울 길이 없다). 항목이 원래 없었으면 지운 것으로 본다.
/// 토큰은 지웠는데 목록을 고치지 못했으면 그 안내를 돌려준다: 목록이 깨졌으면 목록 파일은 건드리지 않고(빈 목록으로 덮어쓰면
/// 다른 계정의 표시 정보를 잃는다, 깨진 파일은 다음 저장·전환이 옆으로 옮긴다), 목록을 못 쓰면 그 오류를 붙인다
public func removeAccount(_ id: String, ports: AccountPorts) throws -> String? {
    let item = KeychainItem.savedAccount(id)
    ports.keychain.delete(item)
    guard !ports.keychain.exists(item) else { throw AccountError(removeKeychainFailed) }
    guard let entries = writableSavedAccountEntries(ports.saved.read()) else { return removeListBroken }
    do {
        try ports.saved.write(savedAccountsData(removingSavedAccount(entries, id: id)))
    } catch {
        return removeListWriteFailed + error.localizedDescription
    }
    return nil
}

// 키체인 항목 내용(JSON 객체) — 없거나 JSON 객체가 아니면 nil
private func credentialsJSON(_ raw: Data?) -> [String: Any]? {
    raw.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
}
