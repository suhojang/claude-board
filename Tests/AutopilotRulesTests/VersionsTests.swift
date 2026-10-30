// 기술 스택 버전 목록 규칙(Sources/AutopilotRules/Versions.swift) 시험 — 2026-10-30 2단계 5-2b 에서 새로 씀.
// 응답 꼴은 그날 받은 공식 응답(start.spring.io v2.3·endoflife.date v1·npm dist-tags·PyPI)을 판정에 쓰는 칸만 남겨 줄인 것이다.
// 불러오기는 가짜 받기·캐시·시계로 캐시 하루·받기 실패·앱에 든 목록 갈래를 본다
import Foundation
import Testing
import AutopilotRules

private func data(_ s: String) -> Data { Data(s.utf8) }
private func eol(_ rel: [(String, Bool, Bool, String?, String?)], java: [String: String] = [:]) -> Data {
    let r = rel.map { n, lts, gone, from, latest -> [String: Any] in
        var o: [String: Any] = ["name": n, "isLts": lts, "isEol": gone, "eolFrom": from.map { $0 as Any } ?? NSNull()]
        if let latest { o["latest"] = ["name": latest] }
        if let j = java[n] { o["custom"] = ["supportedJavaVersions": j] }
        return o
    }
    return try! JSONSerialization.data(withJSONObject: ["schema_version": "1.2.0", "result": ["name": "x", "releases": r]])
}
// 2026-10-30 응답을 줄인 것
private let springMeta = data(#"""
{"bootVersion":{"default":"4.1.2","values":[{"id":"4.2.0-SNAPSHOT"},{"id":"4.2.0-RC1"},{"id":"4.1.3-SNAPSHOT"},{"id":"4.1.2"},{"id":"4.0.10-SNAPSHOT"},{"id":"4.0.9"}]},
 "javaVersion":{"default":"17","values":[{"id":"27"},{"id":"25"},{"id":"21"},{"id":"17"}]}}
"""#)
private let feedsToday: [APFeed: Data] = [
    .spring: springMeta,
    .temurin: eol([("27", false, false, "2027-03-23", "27.0.1"), ("25", true, false, "2031-09-30", "25.0.5"), ("21", true, false, "2029-12-31", "21.0.13"),
                   ("17", true, false, "2027-10-31", "17.0.21"), ("11", true, false, "2027-10-31", "11.0.32"), ("8", true, false, "2030-12-31", "8u504")]),
    .springBoot: eol([("4.1", false, false, "2027-07-31", "4.1.2"), ("4.0", false, false, "2026-12-31", "4.0.9"), ("3.5", false, true, "2026-06-30", "3.5.9")],
                     java: ["4.1": "17 - 26", "4.0": "17 - 25"]),
    .node: eol([("27", false, false, "2027-06-01", "27.0.0"), ("26", true, false, "2029-04-30", "26.13.0"), ("24", true, false, "2028-04-30", "24.22.2"),
                ("22", true, false, "2027-04-30", "22.23.4"), ("20", true, true, "2026-04-30", "20.19.0")]),
    .python: eol([("3.15", false, false, "2031-10-31", "3.15.2"), ("3.14", false, false, "2030-10-31", "3.14.10"), ("3.13", false, false, "2029-10-31", "3.13.18"),
                  ("3.12", false, false, "2028-10-31", "3.12.16")]),
    .postgresql: eol([("18", false, false, "2030-11-14", "18.6"), ("17", false, false, "2029-11-08", "17.11"), ("16", false, false, "2028-11-09", "16.15"),
                      ("15", false, false, "2027-11-11", "15.19")]),
    .mysql: eol([("26.10", false, false, nil, "26.10.0"), ("9.7", true, false, "2034-04-30", "9.7.4"), ("8.4", true, false, "2032-04-30", "8.4.13")]),
    .kotlin: eol([("2.4", false, false, nil, "2.4.21")]),
    .nest: data(#"{"latest":"12.3.2","legacy":"11.2.8","next":"12.0.0-alpha.7"}"#),
    .react: data(#"{"latest":"19.3.1","canary":"19.3.0-canary"}"#),
    .next: data(#"{"latest":"16.4.3"}"#),
    .vue: data(#"{"latest":"3.5.44"}"#),
    .vite: data(#"{"latest":"8.3.5"}"#),
    .fastapi: data(#"{"info":{"version":"0.162.0"}}"#),
]

private func show(_ c: APCatalog, _ k: String) -> String {
    c.list(k).map { $0.v + ($0.lts ? " LTS" : "") + ($0.rec ? " 추천" : "") }.joined(separator: ", ")
}

struct CatalogRow: Row {
    let note: String
    let key: String
    let want: String
}
private let catalogRows: [CatalogRow] = [
    .init(note: "Spring Boot: GA 만(SNAPSHOT·RC 뺌), 지원 중인 줄, 추천은 start.spring.io 기본값", key: "spring-boot", want: "4.1.2 추천, 4.0.9"),
    .init(note: "Java: 지원 중인 LTS ∩ start.spring.io ∩ Spring Boot 4.1 범위(17 - 26) — 27(LTS 아님)·11·8(start.spring.io 에 없음) 뺌, 기본값 17 대신 최신 LTS 25 추천",
          key: "java", want: "25 LTS 추천, 21 LTS, 17 LTS"),
    .init(note: "Node.js: LTS 줄이 있으면 LTS 만(27 은 LTS 아님, 20 은 지원 끝), 최신 LTS 추천", key: "node", want: "26 LTS 추천, 24 LTS, 22 LTS"),
    .init(note: "Python: LTS 개념이 없어 지원 중인 최신 셋, 최신 추천", key: "python", want: "3.15 추천, 3.14, 3.13"),
    .init(note: "PostgreSQL: 지원 중인 최신 셋", key: "postgresql", want: "18 추천, 17, 16"),
    .init(note: "MySQL: LTS 줄이 있으면 LTS 만(26.10 혁신판 뺌)", key: "mysql", want: "9.7 LTS 추천, 8.4 LTS"),
    .init(note: "Kotlin: 지원 끝 날이 없는 지원 중인 줄", key: "kotlin", want: "2.4 추천"),
    .init(note: "NestJS: npm latest 추천과 앞 큰 판(legacy)", key: "nest", want: "12.3 추천, 11.2"),
    .init(note: "React·Vite·Next.js·Vue·FastAPI 는 latest 하나", key: "react", want: "19.3 추천"),
    .init(note: "Vite", key: "vite", want: "8.3 추천"),
    .init(note: "Next.js", key: "next", want: "16.4 추천"),
    .init(note: "Vue", key: "vue", want: "3.5 추천"),
    .init(note: "FastAPI(PyPI)", key: "fastapi", want: "0.162 추천"),
]

@Suite("자율주행 기술 스택: 버전 목록") struct CatalogTests {
    @Test("공식 응답으로 만든 목록과 추천", arguments: catalogRows)
    func 표(_ r: CatalogRow) {
        let c = apBuildCatalog(feedsToday, at: 1)!
        #expect(show(c, r.key) == r.want)
    }

    @Test("지원 끝 달과 Java 메모")
    func 메모() {
        let c = apBuildCatalog(feedsToday, at: 1)!
        #expect(c.list("java").first?.until == "2031-09")
        #expect(c.list("spring-boot").last?.until == "2026-12")
        #expect(c.notes["java"] == "목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 26 밖)")
        #expect(c.recommended("java") == "25" && c.recommended("nope") == nil)
    }

    @Test("응답 하나라도 없거나 꼴이 다르면 만들지 않는다")
    func 실패() {
        var g = feedsToday
        g[.vue] = nil
        #expect(apBuildCatalog(g, at: 1) == nil)
        g = feedsToday
        g[.spring] = data(#"{"bootVersion":{"values":[{"id":"4.2.0-SNAPSHOT"}]},"javaVersion":{"values":[{"id":"25"}]}}"#)   // GA 없음
        #expect(apBuildCatalog(g, at: 1) == nil)
        g = feedsToday
        g[.temurin] = data("<html>")
        #expect(apBuildCatalog(g, at: 1) == nil)
        g = feedsToday
        g[.nest] = data(#"{"next":"1.0.0"}"#)   // latest 없음
        #expect(apBuildCatalog(g, at: 1) == nil)
    }

    @Test("앱에 든 목록은 풀리고 같은 꼴이다")
    func 앱목록() {
        let c = apCatalog(apBuiltinCatalogData)
        #expect(c != nil)
        for k in ["java", "spring-boot", "kotlin", "node", "nest", "python", "fastapi", "react", "vite", "next", "vue", "postgresql", "mysql"] {
            #expect(c?.list(k).isEmpty == false, "항목 \(k)")
            #expect(c?.list(k).filter(\.rec).count == 1, "추천 하나 \(k)")
        }
    }

    @Test("받을 곳은 고정한 공개 주소뿐(https, 사용자 자료 없음)")
    func 주소() {
        for f in APFeed.allCases {
            #expect(f.url.hasPrefix("https://start.spring.io/") || f.url.hasPrefix("https://endoflife.date/api/v1/products/")
                    || f.url.hasPrefix("https://registry.npmjs.org/-/package/") || f.url.hasPrefix("https://pypi.org/pypi/"))
            #expect(!f.url.contains("?"))
        }
        #expect(APFeed.spring.accept == "application/vnd.initializr.v2.3+json")
    }
}

// MARK: - 불러오기(하루 캐시·받기 실패·앱에 든 목록)

private final class FakeFeeds: APVersionFeeds, @unchecked Sendable {
    var got: [APFeed: Data]
    var asked: [APFeed] = []
    init(_ g: [APFeed: Data]) { got = g }
    func fetch(_ feed: APFeed) -> Data? { asked.append(feed); return got[feed] }
}
private final class FakeCache: APVersionCache, @unchecked Sendable {
    var data: Data?
    var at: Date?
    var wrote: [Data] = []
    init(_ c: APCatalog?, at: Date?) { data = c.map(apCatalogData); self.at = at }
    func read() -> (data: Data, at: Date)? { data.flatMap { d in at.map { (d, $0) } } }
    func write(_ data: Data) { wrote.append(data); self.data = data }
}
private struct FakeClock: APClock { let now: Date }

struct LoadRow: Row {
    let note: String
    let cacheAge: Double?        // 초(nil 이면 캐시 없음)
    let feeds: Bool              // 받기를 켰나
    let feedsOK: Bool
    let want: APCatalogOrigin
    let asks: Bool               // 받기를 불렀나
    let writes: Bool
}
private let loadRows: [LoadRow] = [
    .init(note: "하루 안의 캐시면 받지 않는다", cacheAge: 3600, feeds: true, feedsOK: true, want: .cached, asks: false, writes: false),
    .init(note: "하루 지난 캐시면 새로 받아 캐시에 쓴다", cacheAge: 90000, feeds: true, feedsOK: true, want: .fresh, asks: true, writes: true),
    .init(note: "캐시가 없으면 받는다", cacheAge: nil, feeds: true, feedsOK: true, want: .fresh, asks: true, writes: true),
    .init(note: "받지 못하면 하루 지난 캐시(오래된 목록)", cacheAge: 90000, feeds: true, feedsOK: false, want: .stale, asks: true, writes: false),
    .init(note: "받지 못했고 캐시도 없으면 앱에 든 목록", cacheAge: nil, feeds: true, feedsOK: false, want: .builtin, asks: true, writes: false),
    .init(note: "받기를 끄면 하루 지난 캐시", cacheAge: 90000, feeds: false, feedsOK: true, want: .stale, asks: false, writes: false),
    .init(note: "받기를 끄고 캐시도 없으면 앱에 든 목록", cacheAge: nil, feeds: false, feedsOK: true, want: .builtin, asks: false, writes: false),
    .init(note: "캐시 시각이 앞날이면(시계를 되돌림) 믿지 않고 받는다", cacheAge: -7200, feeds: true, feedsOK: true, want: .fresh, asks: true, writes: true),
]

@Suite("자율주행 기술 스택: 버전 목록 불러오기") struct CatalogLoadTests {
    @Test("하루 캐시와 받기 실패 갈래", arguments: loadRows)
    func 표(_ r: LoadRow) {
        let now = Date(timeIntervalSince1970: 1_793_300_000)
        let old = APCatalog(at: 1, items: ["java": [APVer(v: "21", lts: true, rec: true)]])
        let cache = FakeCache(r.cacheAge == nil ? nil : old, at: r.cacheAge.map { now.addingTimeInterval(-$0) })
        var g = feedsToday
        if !r.feedsOK { g[.fastapi] = nil }
        let feeds = FakeFeeds(g)
        let l = apLoadCatalog(feeds: r.feeds ? feeds : nil, cache: cache, clock: FakeClock(now: now), builtin: apBuiltinCatalogData)
        #expect(l?.origin == r.want)
        #expect(!feeds.asked.isEmpty == r.asks)
        #expect(!cache.wrote.isEmpty == r.writes)
        if r.want == .fresh { #expect(l?.catalog.at == now.timeIntervalSince1970 && l?.catalog.recommended("java") == "25") }
        if r.want == .stale { #expect(l?.catalog == old && l?.failed.isEmpty == false) }
        if !r.feedsOK && r.feeds { #expect(l?.failed == ["PyPI"]) }
    }

    @Test("머리 글")
    func 머리() {
        let c = APCatalog(at: 1, items: [:])
        #expect(apCatalogLabel(APCatalogLoad(catalog: c, origin: .fresh), date: "오늘 09:12") == "버전 목록 오늘 09:12 받음 · 공식 목록")
        #expect(apCatalogLabel(APCatalogLoad(catalog: c, origin: .builtin), date: "10월 30일") == "오래된 목록(10월 30일) · 받지 못해 앱에 든 목록을 씁니다")
    }
}

