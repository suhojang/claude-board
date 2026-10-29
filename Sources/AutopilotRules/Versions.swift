// 자율주행 규칙(AutopilotRules) — 기술 스택 칸의 버전 목록과 추천(설계 2장 "기술 스택": 공식 목록, 안 되면 앱 안 고정 목록, 하루 캐시).
// 2026-10-30 2단계 5-2b 에서 새로 씀. 받기는 포트(APVersionFeeds — 고정한 공개 주소 APFeed 만, 사용자 자료를 보내지 않음)로, 캐시는 APVersionCache,
// 시각은 APClock 으로만 부른다. 응답 풀기·목록 만들기·추천은 값으로 한다.
// 추천(설계 판단): 그 프레임워크가 받치는 최신 LTS, LTS 개념이 없으면 지원 중인 최신 안정판. start.spring.io 의 Java 기본값(17)은 따르지 않는다
import Foundation

// 받을 곳 — 닫힌 목록이라 규칙·어댑터가 이 밖의 주소를 부를 수 없다(공개 읽기만)
public enum APFeed: String, CaseIterable, Sendable {
    case spring, temurin, springBoot, node, python, postgresql, mysql, kotlin, nest, react, next, vue, vite, fastapi
    public var url: String {
        switch self {
        case .spring: "https://start.spring.io/"
        case .temurin: "https://endoflife.date/api/v1/products/eclipse-temurin"
        case .springBoot: "https://endoflife.date/api/v1/products/spring-boot"
        case .node: "https://endoflife.date/api/v1/products/nodejs"
        case .python: "https://endoflife.date/api/v1/products/python"
        case .postgresql: "https://endoflife.date/api/v1/products/postgresql"
        case .mysql: "https://endoflife.date/api/v1/products/mysql"
        case .kotlin: "https://endoflife.date/api/v1/products/kotlin"
        case .nest: "https://registry.npmjs.org/-/package/@nestjs/core/dist-tags"
        case .react: "https://registry.npmjs.org/-/package/react/dist-tags"
        case .next: "https://registry.npmjs.org/-/package/next/dist-tags"
        case .vue: "https://registry.npmjs.org/-/package/vue/dist-tags"
        case .vite: "https://registry.npmjs.org/-/package/vite/dist-tags"
        case .fastapi: "https://pypi.org/pypi/fastapi/json"
        }
    }
    // start.spring.io 는 이 Accept 일 때 메타데이터(JSON)를 준다
    public var accept: String { self == .spring ? "application/vnd.initializr.v2.3+json" : "application/json" }
    public var label: String {
        switch self {
        case .spring: "start.spring.io"
        case .nest, .react, .next, .vue, .vite: "npm"
        case .fastapi: "PyPI"
        default: "endoflife.date"
        }
    }
}

// 목록의 판 하나
public struct APVer: Equatable, Sendable, Codable {
    public var v: String          // 보이는 판(예: 25, 4.1.2, 19.3)
    public var lts = false
    public var until: String?     // 지원 끝(yyyy-MM)
    public var rec = false        // 추천
    public init(v: String, lts: Bool = false, until: String? = nil, rec: Bool = false) { self.v = v; self.lts = lts; self.until = until; self.rec = rec }
}

// 버전 목록 — 항목 키(java·spring-boot·kotlin·node·nest·python·fastapi·react·vite·next·vue·postgresql·mysql)마다 판 목록, 받은 시각, 항목 메모
public struct APCatalog: Equatable, Sendable, Codable {
    public var at: Double
    public var items: [String: [APVer]]
    public var notes: [String: String] = [:]
    public init(at: Double, items: [String: [APVer]], notes: [String: String] = [:]) { self.at = at; self.items = items; self.notes = notes }
    public func list(_ key: String) -> [APVer] { items[key] ?? [] }
    public func recommended(_ key: String) -> String? { (items[key]?.first { $0.rec } ?? items[key]?.first)?.v }
}

// MARK: - 응답 풀기

public struct APSpringMeta: Equatable, Sendable {
    public var boots: [String] = []     // GA 만(-SNAPSHOT·-RC·-M 은 뺌), 응답 차례(최신부터)
    public var bootDefault: String?
    public var javas: [String] = []
    public init() {}
}
public func apParseSpring(_ d: Data) -> APSpringMeta? {
    guard let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any],
          let b = j["bootVersion"] as? [String: Any], let jv = j["javaVersion"] as? [String: Any] else { return nil }
    var m = APSpringMeta()
    m.boots = (b["values"] as? [[String: Any]] ?? []).compactMap { $0["id"] as? String }.filter { !$0.contains("-") && !$0.contains("SNAPSHOT") }
    m.bootDefault = (b["default"] as? String).flatMap { $0.contains("-") ? nil : $0 }
    m.javas = (jv["values"] as? [[String: Any]] ?? []).compactMap { $0["id"] as? String }
    return m.boots.isEmpty || m.javas.isEmpty ? nil : m
}

public struct APEolRelease: Equatable, Sendable {
    public var name: String
    public var lts = false
    public var eol = false
    public var eolFrom: String?
    public var latest: String?
    public var supportedJava: String?   // spring-boot 의 custom.supportedJavaVersions("17 - 26")
    public init(name: String, lts: Bool = false, eol: Bool = false, eolFrom: String? = nil, latest: String? = nil, supportedJava: String? = nil) {
        self.name = name; self.lts = lts; self.eol = eol; self.eolFrom = eolFrom; self.latest = latest; self.supportedJava = supportedJava
    }
}
public func apParseEol(_ d: Data) -> [APEolRelease]? {
    guard let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any],
          let rel = (j["result"] as? [String: Any])?["releases"] as? [[String: Any]] else { return nil }
    let out = rel.compactMap { r -> APEolRelease? in
        guard let n = r["name"] as? String else { return nil }
        return APEolRelease(name: n, lts: r["isLts"] as? Bool ?? false, eol: r["isEol"] as? Bool ?? false, eolFrom: r["eolFrom"] as? String,
                            latest: (r["latest"] as? [String: Any])?["name"] as? String,
                            supportedJava: (r["custom"] as? [String: Any])?["supportedJavaVersions"] as? String)
    }
    return out.isEmpty ? nil : out
}
public func apParseNpmTags(_ d: Data) -> [String: String]? {
    guard let j = (try? JSONSerialization.jsonObject(with: d)) as? [String: Any], let l = j["latest"] as? String, !l.isEmpty else { return nil }
    return j.compactMapValues { $0 as? String }
}
public func apParsePyPI(_ d: Data) -> String? {
    ((try? JSONSerialization.jsonObject(with: d)) as? [String: Any]).flatMap { ($0["info"] as? [String: Any])?["version"] as? String }
}

// MARK: - 목록 만들기

private func majorMinor(_ v: String) -> String { v.split(separator: ".").prefix(2).joined(separator: ".") }
private func month(_ d: String?) -> String? { d.map { String($0.prefix(7)) } }
private func num(_ s: String) -> Double { Double(s.split(separator: ".").prefix(2).joined(separator: ".")) ?? 0 }

// endoflife 목록에서 보일 판 — 지원 중(isEol 아님)인 것, LTS 줄이 있으면 LTS 만(설계: 최신 LTS 추천), 최신부터 셋. 맨 앞이 추천
public func apEolList(_ rel: [APEolRelease], limit: Int = 3) -> [APVer] {
    let live = rel.filter { !$0.eol }
    let keep = live.contains(where: \.lts) ? live.filter(\.lts) : live
    return keep.prefix(limit).enumerated().map { i, r in APVer(v: r.name, lts: r.lts, until: month(r.eolFrom), rec: i == 0) }
}

// Spring Boot 가 받치는 Java 범위("17 - 26") 안에 드는가
private func inRange(_ java: String, _ range: String?) -> Bool {
    guard let r = range else { return true }
    let p = r.split(separator: "-").map { Double($0.trimmingCharacters(in: .whitespaces)) ?? 0 }
    guard p.count == 2, let j = Double(java) else { return true }
    return j >= p[0] && j <= p[1]
}

// 받은 응답들로 목록을 만든다. 하나라도 없거나 풀 수 없으면 nil(앞 목록으로 간다)
public func apBuildCatalog(_ got: [APFeed: Data], at: Double) -> APCatalog? {
    guard let sd = got[.spring], let spring = apParseSpring(sd),
          let tem = got[.temurin].flatMap(apParseEol), let sb = got[.springBoot].flatMap(apParseEol),
          let node = got[.node].flatMap(apParseEol), let py = got[.python].flatMap(apParseEol), let pg = got[.postgresql].flatMap(apParseEol),
          let my = got[.mysql].flatMap(apParseEol), let kt = got[.kotlin].flatMap(apParseEol),
          let nest = got[.nest].flatMap(apParseNpmTags), let react = got[.react].flatMap(apParseNpmTags), let next = got[.next].flatMap(apParseNpmTags),
          let vue = got[.vue].flatMap(apParseNpmTags), let vite = got[.vite].flatMap(apParseNpmTags), let fast = got[.fastapi].flatMap(apParsePyPI) else { return nil }
    var items: [String: [APVer]] = [:], notes: [String: String] = [:]
    // Spring Boot: GA 판 가운데 지원 중인 줄(endoflife 의 4.1·4.0 …)만. 추천은 start.spring.io 기본값(GA 일 때), 아니면 맨 앞
    var boots: [APVer] = []
    for b in spring.boots {
        guard let r = sb.first(where: { $0.name == majorMinor(b) }), !r.eol else { continue }
        if !boots.contains(where: { majorMinor($0.v) == majorMinor(b) }) { boots.append(APVer(v: b, until: month(r.eolFrom))) }
    }
    guard !boots.isEmpty else { return nil }
    let recBoot = boots.firstIndex { $0.v == spring.bootDefault } ?? 0
    boots[recBoot].rec = true
    items["spring-boot"] = Array(boots.prefix(3))
    // Java: start.spring.io 가 고르게 하는 판 ∩ Temurin 의 지원 중인 LTS ∩ 추천 Spring Boot 가 받치는 범위. 추천은 그 가운데 최신
    let range = sb.first { $0.name == majorMinor(boots[recBoot].v) }?.supportedJava
    let lts = tem.filter { $0.lts && !$0.eol }
    var java: [APVer] = []
    for j in spring.javas {
        guard let t = lts.first(where: { $0.name == j }), inRange(j, range) else { continue }
        java.append(APVer(v: j, lts: true, until: month(t.eolFrom)))
    }
    java.sort { num($0.v) > num($1.v) }
    guard !java.isEmpty else { return nil }
    java[0].rec = true
    items["java"] = Array(java.prefix(3))
    let left = spring.javas.filter { j in !java.contains { $0.v == j } }
    if !left.isEmpty {
        notes["java"] = "목록에 없는 판: Java " + left.joined(separator: "·") + "(LTS 가 아니거나 Spring Boot \(majorMinor(boots[recBoot].v)) 의 범위 \(range ?? "모름") 밖)"
    }
    items["kotlin"] = apEolList(kt, limit: 2)
    items["node"] = apEolList(node)
    items["python"] = apEolList(py)
    items["postgresql"] = apEolList(pg)
    items["mysql"] = apEolList(my, limit: 2)
    // npm: latest 를 추천으로, NestJS 는 앞 큰 판(legacy)도
    var n = [APVer(v: majorMinor(nest["latest"] ?? ""), rec: true)]
    if let l = nest["legacy"], majorMinor(l) != n[0].v { n.append(APVer(v: majorMinor(l))) }
    items["nest"] = n
    for (k, t) in [("react", react), ("next", next), ("vue", vue), ("vite", vite)] { items[k] = [APVer(v: majorMinor(t["latest"] ?? ""), rec: true)] }
    items["fastapi"] = [APVer(v: majorMinor(fast), rec: true)]
    return APCatalog(at: at, items: items, notes: notes)
}

public func apCatalogData(_ c: APCatalog) -> Data {
    let e = JSONEncoder()
    e.outputFormatting = [.sortedKeys]
    return (try? e.encode(c)) ?? Data()
}
public func apCatalog(_ d: Data) -> APCatalog? { try? JSONDecoder().decode(APCatalog.self, from: d) }

// MARK: - 불러오기

public enum APCatalogOrigin: String, Equatable, Sendable {
    case fresh      // 지금 공식 목록을 받음
    case cached     // 하루 안에 받은 캐시
    case stale      // 받지 못해 하루 지난 캐시(오래된 목록)
    case builtin    // 받지 못했고 캐시도 없어 앱에 든 목록(오래된 목록)
}
public struct APCatalogLoad: Equatable, Sendable {
    public var catalog: APCatalog
    public var origin: APCatalogOrigin
    public var failed: [String] = []    // 받지 못한 곳(주소 이름)
    public init(catalog: APCatalog, origin: APCatalogOrigin, failed: [String] = []) { self.catalog = catalog; self.origin = origin; self.failed = failed }
}

// 유스케이스: 버전 목록 불러오기 — 하루 안의 캐시면 그것, 아니면 공식 목록을 모두 받아 만들고 캐시에 쓴다. 받거나 만들지 못하면
// 하루 지난 캐시(오래된 목록), 그것도 없으면 앱에 든 목록. feeds 가 nil 이면 받지 않는다(받기를 끈 시험 빌드 등)
public func apLoadCatalog(feeds: APVersionFeeds?, cache: APVersionCache, clock: APClock, builtin: Data) -> APCatalogLoad? {
    let now = clock.now
    let cached = cache.read().flatMap { c in apCatalog(c.data).map { ($0, c.at) } }
    if let (c, at) = cached, now.timeIntervalSince(at) < 86400, now >= at { return APCatalogLoad(catalog: c, origin: .cached) }
    var failed: [String] = []
    if let feeds {
        var got: [APFeed: Data] = [:]
        for f in APFeed.allCases {
            if let d = feeds.fetch(f) { got[f] = d } else if !failed.contains(f.label) { failed.append(f.label) }
        }
        if failed.isEmpty, let c = apBuildCatalog(got, at: now.timeIntervalSince1970) {
            cache.write(apCatalogData(c))
            return APCatalogLoad(catalog: c, origin: .fresh)
        }
        if failed.isEmpty { failed.append("응답 꼴이 달라 풀지 못함") }
    } else {
        failed.append("받기 꺼짐")
    }
    if let (c, _) = cached { return APCatalogLoad(catalog: c, origin: .stale, failed: failed) }
    return apCatalog(builtin).map { APCatalogLoad(catalog: $0, origin: .builtin, failed: failed) }
}

// 목록 머리 글 — "버전 목록 오늘 09:12 받음 · 공식 목록" / "오래된 목록(10월 30일) · 앱에 든 목록"(날짜 글자는 보드가 만든다)
public func apCatalogLabel(_ l: APCatalogLoad, date: String) -> String {
    switch l.origin {
    case .fresh, .cached: "버전 목록 \(date) 받음 · 공식 목록"
    case .stale: "오래된 목록(\(date)) · 받지 못해 앞서 받은 목록을 씁니다"
    case .builtin: "오래된 목록(\(date)) · 받지 못해 앱에 든 목록을 씁니다"
    }
}
