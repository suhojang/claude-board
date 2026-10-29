// 요금제 사용량 규칙 — 사용량 API 응답 해석, 로그인 정보의 토큰 상태, 캐시를 믿을지·새로 받을지, 받은 결과 처리
// (UsageStore 에 있던 것을 옮김, 동작은 그대로). 바깥(사용량 캐시·로그인 정보·사용량 API·시계)은 포트(Ports.swift)로만 부른다.
// 옮긴 뒤 고친 것(이슈 #9 2차): 못 푸는 캐시와 수정 시각이 미래인 캐시는 받고, 푼 한도가 하나도 없는 응답은 실패로 본다.
// (3차): 모델별 주간의 id 가 겹치면 뒤의 것에 -2·-3 … 을 붙이고, limits 에 섞인 객체 아닌 원소는 그것만 뺀다.
//
// 모양: 유스케이스 셋 — 캐시 확인(checkUsageCache, 메인) → 받기(fetchUsage, 백그라운드) → 받은 결과 처리(settleUsage, 메인).
// 받기와 처리 사이에 앱의 계정 전환 세대 확인이 낀다: 받는 사이 계정을 바꿨으면 받은 값은 앞 계정 것이라 캐시에 쓰지도 않고 버린다.
// 그래서 캐시 쓰기가 든 처리는 받기와 한 함수로 묶지 않고, 세대 확인 뒤에 앱이 부른다.
// 상태(limits·failed·tokenExpired·trustCacheAfter·fetching·generation)는 앱(UsageStore)이 쥐고 규칙은 바꿀 값만 돌려준다 —
// limits 는 SwiftUI 애니메이션과 함께 @Published 로 바뀌어야 해서, 규칙 쪽에 상태 기계를 따로 두면 같은 상태가 둘이 되고
// @Published 를 다시 쓰는 때(화면이 다시 그려지는 때)가 달라진다
import Foundation

// 요금제 사용 한도: 5시간·주간(전체)·주간(모델별 — 예: Fable)
public struct Limit: Equatable, Identifiable, Sendable {
    public let id: String
    public let label: String
    public let pct: Double
    public let resets: Date?

    public init(id: String, label: String, pct: Double, resets: Date?) {
        self.id = id; self.label = label; self.pct = pct; self.resets = resets
    }

    // 간략 모드 이름 — 5시간 · 주간 · 모델 이름(Fable). 더 좁으면 5h · 7d · 첫 글자
    public var short: String {
        id == "5h" ? "5시간" : id == "7d" ? "주간" : (label.components(separatedBy: "·").last ?? label).trimmingCharacters(in: .whitespaces)
    }
    public var tiny: String { id == "5h" ? "5h" : id == "7d" ? "7d" : String(short.prefix(1)) }
}

// MARK: - 사용량 API 응답 해석

/// 사용량 API 응답(캐시 파일도 같은 형식)을 한도 목록으로 푼다. JSON 이 아니거나 five_hour 가 없으면 nil(실패).
/// 항목마다 값이 숫자가 아니면 그 항목만 뺀다. 푼 한도가 하나도 없어도 nil(실패) — 빈 "성공" 을 적용하고 캐시에 쓰면
/// 보이던 한도가 사라지고 상태줄도 그 빈 캐시를 믿는다. 5시간을 못 풀어도 다른 한도가 풀리면 성공이다.
/// 차례: 5시간 → 주간 전체 → limits 의 weekly_scoped(모델별, 응답 차례). limits 는 객체인 원소만 본다(다른 원소가 섞여도 그것만 뺀다).
/// id 는 화면 목록(ForEach)의 id 라 유일해야 한다 — 모델별 주간의 이름이 같거나 이름이 없어 둘 다 "모델" 이면 7d-<이름> 이 겹치므로,
/// 뺀 항목을 빼고 앞에서부터 처음 것은 그대로, 뒤의 것은 -2·-3 … 을 붙여 아직 없는 id 로 한다. 라벨은 그대로 둔다
public func parseUsage(_ data: Data) -> [Limit]? {
    guard let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let five = j["five_hour"] as? [String: Any] else { return nil }
    var out: [Limit] = []
    var ids = Set<String>()
    func add(_ id: String, _ label: String, _ o: [String: Any]?, _ key: String) {
        guard let o, let v = (o[key] as? NSNumber)?.doubleValue else { return }
        var unique = id, n = 2
        while ids.contains(unique) { unique = "\(id)-\(n)"; n += 1 }
        ids.insert(unique)
        out.append(Limit(id: unique, label: label, pct: v, resets: isoDate(o["resets_at"] as? String)))
    }
    add("5h", "5시간", five, "utilization")
    add("7d", "주간 · 전체", j["seven_day"] as? [String: Any], "utilization")
    let scoped = (j["limits"] as? [Any] ?? []).compactMap { $0 as? [String: Any] }
    for l in scoped where l["kind"] as? String == "weekly_scoped" {
        let name = ((l["scope"] as? [String: Any])?["model"] as? [String: Any])?["display_name"] as? String ?? "모델"
        add("7d-\(name)", "주간 · \(name)", l, "percent")
    }
    return out.isEmpty ? nil : out
}

// 날짜 형식기는 만들기가 비싸다 — 매초 다시 그리는 줄마다 새로 만들지 않게 한 번만 만든다
private let isoFractional: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return f
}()
private let isoPlain = ISO8601DateFormatter()

/// ISO8601 시각 — 소수 초가 있는 것(사용량 API 는 마이크로초까지 준다, 밀리초까지 읽힌다)과 없는 것 모두. 보드 앱의 대화 기록 시각도 이것으로 푼다
public func isoDate(_ s: String?) -> Date? {
    guard let s else { return nil }
    return isoFractional.date(from: s) ?? isoPlain.date(from: s)
}

// MARK: - 로그인 정보의 토큰 상태

/// Claude Code 로그인 정보(키체인 JSON)에서 가른 토큰 상태. 토큰은 메모리에만 둔다 — 글자로 찍으면 토큰을 가린다
public enum LoginToken: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    case missing            // 로그인 정보가 없거나 JSON 이 아니거나 claudeAiOauth.accessToken 이 없다
    case expired            // expiresAt(밀리초)이 지났다 — 세션이 다음 요청을 보내며 갱신해야 받을 수 있다
    case valid(String)      // 그 토큰으로 사용량을 받는다

    public var description: String {
        switch self {
        case .missing: "missing"
        case .expired: "expired"
        case .valid: "valid(가림)"
        }
    }
    public var debugDescription: String { description }
}

/// 토큰이 없으면 만료를 보지 않는다(없음이 먼저). expiresAt 이 없거나 숫자가 아니면 유효로 보고, 같은 밀리초는 아직 유효하다
public func loginToken(_ raw: Data?, now: Date) -> LoginToken {
    guard let raw,
          let j = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
          let oauth = j["claudeAiOauth"] as? [String: Any],
          let token = oauth["accessToken"] as? String else { return .missing }
    if let exp = (oauth["expiresAt"] as? NSNumber)?.doubleValue, exp / 1000 < now.timeIntervalSince1970 { return .expired }
    return .valid(token)
}

// MARK: - 유스케이스: 캐시 확인 (메인, 30초마다)

public struct UsageCacheCheck: Equatable, Sendable {
    public let cached: [Limit]?   // 믿을 수 있는 캐시를 풀었으면 그 한도 — 앱이 보인다. 못 믿거나 없거나 깨졌으면 nil
    public let fetch: Bool        // 새로 받을지
    public init(cached: [Limit]?, fetch: Bool) { self.cached = cached; self.fetch = fetch }
}

/// 캐시 파일은 수정 시각이 trustAfter 뒤일 때만 믿는다(같은 시각은 믿지 않는다) — 계정을 바꾼 직후 상태줄이 앞 계정 토큰으로 받던 값을
/// 늦게 써 넣을 수 있다. 내용은 믿을 때만 읽는다. 믿을 캐시가 없거나, 내용을 풀지 못하거나(깨짐·못 읽음·푼 한도 없음),
/// 나이가 음수(수정 시각이 미래, 시계 어긋남)거나 120초를 넘으면 받는다 — 받는 중이면 받지 않는다.
/// 못 푸는 캐시는 믿을 캐시가 없는 것과 같다(나이 .distantPast): 상태줄 스크립트가 같은 파일에 못 읽는 내용을 계속 써도 그 수정 시각에
/// 막혀 120초씩 기다리지 않는다. 그 대신 그런 내용이 계속 쓰이면 확인할 때마다(앱은 30초마다) 받는다.
/// 캐시 적용은 받을지와 따로다(받는 중이어도, 수정 시각이 미래여도 풀리면 적용한다)
public func checkUsageCache(cache: UsageCache, clock: Clock, trustAfter: Date, fetching: Bool) -> UsageCacheCheck {
    let written = cache.modifiedAt().flatMap { $0 > trustAfter ? $0 : nil }
    let cached = written != nil ? cache.read().flatMap(parseUsage) : nil
    let fresh = cached != nil ? written : nil     // 나이를 셀 캐시 — 풀린 것만
    let age = clock.now.timeIntervalSince(fresh ?? .distantPast)
    return UsageCacheCheck(cached: cached, fetch: (age < 0 || age > 120) && !fetching)
}

// MARK: - 유스케이스: 받기 (백그라운드)

public struct UsageFetch: Equatable, Sendable {
    public let data: Data?          // 200 응답 본문. 토큰이 없거나 만료면 API 를 부르지 않아 nil
    public let tokenExpired: Bool
    public init(data: Data?, tokenExpired: Bool) { self.data = data; self.tokenExpired = tokenExpired }
}

/// 로그인 정보의 토큰으로 사용량 API 를 부른다. 토큰이 없거나 만료면 부르지 않는다. 응답은 여기서 풀지 않는다(처리에서 메인 스레드로 푼다)
public func fetchUsage(credentials: LoginCredentials, api: UsageAPI, clock: Clock) -> UsageFetch {
    switch loginToken(credentials.read(), now: clock.now) {
    case .missing: return UsageFetch(data: nil, tokenExpired: false)
    case .expired: return UsageFetch(data: nil, tokenExpired: true)
    case .valid(let token): return UsageFetch(data: api.fetch(token: token), tokenExpired: false)
    }
}

// MARK: - 유스케이스: 받은 결과 처리 (메인, 계정 전환 세대를 확인한 뒤)

public struct UsageSettle: Equatable, Sendable {
    public let limits: [Limit]?     // 받은 응답을 풀었으면 그 한도 — 앱이 보인다. 아니면 nil(지금 한도를 그대로 둔다)
    public let failed: Bool?        // 바꿀 값. nil 이면 건드리지 않는다
    public let tokenExpired: Bool   // 받은 결과의 토큰 상태로 매번 덮어쓴다(200 아님·토큰 없음이면 false)
    public let trustAfter: Date     // 새 trustCacheAfter — 새 토큰으로 직접 받았으면 푼다(.distantPast)
    public init(limits: [Limit]?, failed: Bool?, tokenExpired: Bool, trustAfter: Date) {
        self.limits = limits; self.failed = failed; self.tokenExpired = tokenExpired; self.trustAfter = trustAfter
    }
}

/// 200 이고 풀리면(푼 한도가 하나 이상): 적용하고, 받은 응답 그대로 캐시에 쓰고(상태줄 스크립트가 같은 파일을 읽는다), trustAfter 를 풀고,
/// failed 를 푼다. 아니면(푼 한도가 없는 응답도) 캐시에 쓰지 않고, 지금 한도가 비어 있을 때만 failed — 앞서 보이던 값이 있으면 그대로 둔다
public func settleUsage(_ fetched: UsageFetch, current: [Limit], trustAfter: Date, cache: UsageCache) -> UsageSettle {
    if let data = fetched.data, let limits = parseUsage(data) {
        cache.write(data)
        return UsageSettle(limits: limits, failed: false, tokenExpired: fetched.tokenExpired, trustAfter: .distantPast)
    }
    return UsageSettle(limits: nil, failed: current.isEmpty ? true : nil, tokenExpired: fetched.tokenExpired, trustAfter: trustAfter)
}
