// 자율주행 규칙(AutopilotRules) — 주제로 시작의 기술 스택 칸 셋(프론트·백엔드·DB, 설계 2장 "기술 스택"). 2026-10-30 2단계 5-2b 에서 새로 씀.
// ① 기존 저장소: 빌드·설정 파일을 읽어 "감지됨"(apDetectStack, 파일은 포트 APStackFiles 로만) ② 주제 글: 알려진 이름과 버전 꼴을 낱말 대조로
// 찾아 칸을 미리 채움(apTopicStack, 모델 없이) ③ 기존 저장소인데 주제가 다른 스택을 말하면 경고(apStackConflicts).
// 버전 목록과 추천은 Versions.swift
import Foundation

// 칸 하나 — 이름(목록 이름 또는 감지한 이름), 언어·판, 프레임워크·판, 빌드 도구, 어디서 왔나(감지됨·주제에서 읽음·고름·직접 입력)
public struct APStackSlot: Equatable, Sendable, Codable {
    public var key: String           // 목록 항목 키(spring-java·spring-kotlin·nest·fastapi·react·next·vue·postgresql·mysql), 직접 입력·목록 밖은 ""
    public var name: String          // 사람에게 보이는 이름(예: "Java · Spring Boot")
    public var lang: String = ""
    public var langVersion: String = ""
    public var framework: String = ""
    public var frameworkVersion: String = ""
    public var build: String = ""    // Gradle Groovy·Gradle Kotlin·Maven·npm 등
    public var origin: String        // detected·topic·picked·custom
    public var sources: [String] = []   // 감지한 파일(상대 경로)
    public init(key: String, name: String, lang: String = "", langVersion: String = "", framework: String = "", frameworkVersion: String = "",
                build: String = "", origin: String, sources: [String] = []) {
        self.key = key; self.name = name; self.lang = lang; self.langVersion = langVersion; self.framework = framework
        self.frameworkVersion = frameworkVersion; self.build = build; self.origin = origin; self.sources = sources
    }
}

public struct APStackSet: Equatable, Sendable, Codable {
    public var front: APStackSlot?
    public var back: APStackSlot?
    public var db: APStackSlot?
    public init(front: APStackSlot? = nil, back: APStackSlot? = nil, db: APStackSlot? = nil) { self.front = front; self.back = back; self.db = db }
    public var isEmpty: Bool { front == nil && back == nil && db == nil }
}

// MARK: - 기존 저장소 감지

// 읽을 파일(저장소 뿌리와 흔한 하위 폴더). 이 밖은 읽지 않는다
public let apStackDirs = ["", "backend/", "frontend/", "server/", "client/", "web/", "api/", "app/"]
public let apStackFileNames = ["build.gradle", "build.gradle.kts", "pom.xml", "package.json", "pyproject.toml", "requirements.txt", "go.mod",
                               "src/main/resources/application.yml", "src/main/resources/application.yaml", "src/main/resources/application.properties",
                               "docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml", "prisma/schema.prisma"]
public var apStackCandidates: [String] { apStackDirs.flatMap { d in apStackFileNames.map { d + $0 } } }

private func first(_ s: String, _ pattern: String, group: Int = 1) -> String? {
    guard let re = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]),
          let m = re.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)), m.numberOfRanges > group,
          let r = Range(m.range(at: group), in: s) else { return nil }
    return String(s[r])
}
private func clean(_ v: String?) -> String {
    guard var v = v?.trimmingCharacters(in: .whitespaces) else { return "" }
    while let c = v.first, "^~>=< v".contains(c) { v.removeFirst() }
    return first(v, "^([0-9]+(\\.[0-9]+){0,2})") ?? ""
}
// package.json 의 의존성 판(dependencies·devDependencies)
private func npmDeps(_ text: String) -> [String: String] {
    guard let j = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] else { return [:] }
    var out: [String: String] = [:]
    for k in ["dependencies", "devDependencies", "peerDependencies"] {
        for (n, v) in j[k] as? [String: Any] ?? [:] where out[n] == nil { out[n] = "\(v)" }
    }
    if let e = (j["engines"] as? [String: Any])?["node"] { out["%node"] = "\(e)" }
    return out
}

// 읽은 파일(상대 경로 → 글)에서 칸 셋을 정한다. 칸마다 앞 폴더(뿌리 먼저)에서 처음 찾은 것
public func apDetectStack(_ files: [String: String]) -> APStackSet {
    var set = APStackSet()
    for d in apStackDirs {
        func f(_ n: String) -> String? { files[d + n] }
        // 백엔드: Spring Boot(Gradle·Maven), NestJS, FastAPI, Go
        if set.back == nil {
            if let g = f("build.gradle.kts") ?? f("build.gradle"), g.contains("org.springframework.boot") {
                let kts = f("build.gradle.kts") != nil
                let kotlin = g.contains("org.jetbrains.kotlin") || g.range(of: "kotlin\\(\"(jvm|plugin\\.spring)\"\\)", options: .regularExpression) != nil
                let boot = first(g, "org\\.springframework\\.boot['\"]?\\)?\\s+version\\s+['\"]([0-9][^'\"]*)['\"]")
                let java = first(g, "JavaLanguageVersion\\.of\\(\\s*([0-9]+)\\s*\\)") ?? first(g, "JavaVersion\\.VERSION_([0-9_]+)")?.replacingOccurrences(of: "_", with: ".")
                    ?? first(g, "sourceCompatibility\\s*=\\s*['\"]?([0-9.]+)") ?? first(g, "jvmToolchain\\(\\s*([0-9]+)\\s*\\)")
                set.back = APStackSlot(key: kotlin ? "spring-kotlin" : "spring-java", name: kotlin ? "Kotlin · Spring Boot" : "Java · Spring Boot",
                                       lang: kotlin ? "Kotlin" : "Java", langVersion: clean(java), framework: "Spring Boot", frameworkVersion: clean(boot),
                                       build: kts ? "Gradle Kotlin" : "Gradle Groovy", origin: "detected", sources: [d + (kts ? "build.gradle.kts" : "build.gradle")])
            } else if let p = f("pom.xml"), p.contains("spring-boot") {
                let boot = first(p, "<artifactId>spring-boot-starter-parent</artifactId>\\s*<version>([^<]+)</version>")
                let java = first(p, "<java\\.version>([^<]+)</java\\.version>") ?? first(p, "<maven\\.compiler\\.release>([^<]+)<")
                let kotlin = p.contains("kotlin-maven-plugin")
                set.back = APStackSlot(key: kotlin ? "spring-kotlin" : "spring-java", name: kotlin ? "Kotlin · Spring Boot" : "Java · Spring Boot",
                                       lang: kotlin ? "Kotlin" : "Java", langVersion: clean(java), framework: "Spring Boot", frameworkVersion: clean(boot),
                                       build: "Maven", origin: "detected", sources: [d + "pom.xml"])
            } else if let pj = f("package.json"), let nest = npmDeps(pj)["@nestjs/core"] {
                let deps = npmDeps(pj)
                set.back = APStackSlot(key: "nest", name: "TypeScript · NestJS", lang: "Node.js", langVersion: clean(deps["%node"]), framework: "NestJS",
                                       frameworkVersion: clean(nest), build: "npm", origin: "detected", sources: [d + "package.json"])
            } else if let py = f("pyproject.toml") ?? f("requirements.txt"), py.lowercased().contains("fastapi") {
                let src = f("pyproject.toml") != nil ? "pyproject.toml" : "requirements.txt"
                let ver = first(py, "fastapi(\\[[^\\]]*\\])?\\s*(==|>=|~=|\\^)?\\s*\"?([0-9][0-9.]*)", group: 3)
                let pyv = first(py, "requires-python\\s*=\\s*\"[^0-9]*([0-9.]+)")
                set.back = APStackSlot(key: "fastapi", name: "Python · FastAPI", lang: "Python", langVersion: clean(pyv), framework: "FastAPI",
                                       frameworkVersion: clean(ver), build: src == "pyproject.toml" ? "pyproject" : "pip", origin: "detected", sources: [d + src])
            } else if let go = f("go.mod") {
                let fw = [("github.com/gin-gonic/gin", "Gin"), ("github.com/labstack/echo", "Echo"), ("github.com/gofiber/fiber", "Fiber")].first { go.contains($0.0) }?.1 ?? ""
                set.back = APStackSlot(key: "", name: fw.isEmpty ? "Go" : "Go · \(fw)", lang: "Go", langVersion: clean(first(go, "(?m)^go\\s+([0-9.]+)")),
                                       framework: fw, build: "go", origin: "detected", sources: [d + "go.mod"])
            } else if let pj = f("package.json"), let ex = npmDeps(pj)["express"] {
                set.back = APStackSlot(key: "", name: "Node.js · Express", lang: "Node.js", langVersion: clean(npmDeps(pj)["%node"]), framework: "Express",
                                       frameworkVersion: clean(ex), build: "npm", origin: "detected", sources: [d + "package.json"])
            }
        }
        // 프론트: Next.js, Vue, React(Vite 면 그렇게)
        if set.front == nil, let pj = f("package.json") {
            let deps = npmDeps(pj)
            if let v = deps["next"] {
                set.front = APStackSlot(key: "next", name: "Next.js", framework: "Next.js", frameworkVersion: clean(v), build: "npm", origin: "detected", sources: [d + "package.json"])
            } else if let v = deps["vue"] {
                set.front = APStackSlot(key: "vue", name: "Vue", framework: "Vue", frameworkVersion: clean(v), build: deps["vite"] != nil ? "Vite" : "npm",
                                        origin: "detected", sources: [d + "package.json"])
            } else if let v = deps["react"], deps["react-native"] == nil {
                set.front = APStackSlot(key: "react", name: deps["vite"] != nil ? "React (Vite)" : "React", framework: "React", frameworkVersion: clean(v),
                                        build: deps["vite"] != nil ? "Vite" : "npm", origin: "detected", sources: [d + "package.json"])
            }
        }
        // DB: 설정 파일의 datasource, compose 이미지, prisma provider, 의존성
        if set.db == nil {
            var hit: (String, String, String)?   // (키, 판, 파일)
            for n in ["src/main/resources/application.yml", "src/main/resources/application.yaml", "src/main/resources/application.properties"] {
                guard hit == nil, let t = f(n) else { continue }
                if t.contains("jdbc:postgresql") { hit = ("postgresql", "", n) } else if t.contains("jdbc:mysql") || t.contains("jdbc:mariadb") { hit = ("mysql", "", n) }
            }
            for n in ["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"] {
                guard let t = f(n) else { continue }
                if let v = first(t, "image:\\s*['\"]?(?:[a-z0-9./-]*/)?postgres(?:ql)?:([0-9.]+)") { if hit == nil || hit?.0 == "postgresql" { hit = ("postgresql", v, hit?.2 ?? n) } }
                else if let v = first(t, "image:\\s*['\"]?(?:[a-z0-9./-]*/)?mysql:([0-9.]+)") { if hit == nil || hit?.0 == "mysql" { hit = ("mysql", v, hit?.2 ?? n) } }
                else if hit == nil, t.range(of: "image:\\s*['\"]?(?:[a-z0-9./-]*/)?postgres", options: .regularExpression) != nil { hit = ("postgresql", "", n) }
                else if hit == nil, t.range(of: "image:\\s*['\"]?(?:[a-z0-9./-]*/)?mysql", options: .regularExpression) != nil { hit = ("mysql", "", n) }
            }
            if hit == nil, let t = f("prisma/schema.prisma") {
                if let pv = first(t, "provider\\s*=\\s*\"(postgresql|mysql)\"") { hit = (pv.lowercased(), "", "prisma/schema.prisma") }
            }
            if hit == nil, let pj = f("package.json") {
                let deps = npmDeps(pj)
                if deps["pg"] != nil { hit = ("postgresql", "", "package.json") } else if deps["mysql2"] != nil || deps["mysql"] != nil { hit = ("mysql", "", "package.json") }
            }
            for n in ["pyproject.toml", "requirements.txt"] {
                guard hit == nil, let l = f(n)?.lowercased() else { continue }
                if l.contains("psycopg") || l.contains("asyncpg") { hit = ("postgresql", "", n) }
                else if l.contains("pymysql") || l.contains("mysqlclient") || l.contains("aiomysql") { hit = ("mysql", "", n) }
            }
            if let h = hit {
                set.db = APStackSlot(key: h.0, name: h.0 == "postgresql" ? "PostgreSQL" : "MySQL", framework: h.0 == "postgresql" ? "PostgreSQL" : "MySQL",
                                     frameworkVersion: clean(h.1), origin: "detected", sources: [d + h.2])
            }
        }
    }
    return set
}

// 유스케이스: 기존 저장소의 기술 스택 감지 — 고른 파일만 읽어(APStackFiles) 칸 셋을 정한다
public func apDetectStack(root: String, files: APStackFiles) -> APStackSet {
    var got: [String: String] = [:]
    for rel in apStackCandidates { if let t = files.read(root, rel) { got[rel] = t } }
    return apDetectStack(got)
}

// MARK: - 주제 글 낱말 대조

// 찾은 낱말 하나 — 적힌 그대로(강조용), 어느 칸·무엇인지, 바로 뒤의 버전 꼴
public struct APTopicHit: Equatable, Sendable {
    public let word: String
    public let slot: String          // front·back·db·lang
    public let target: String        // 목록 항목 키 또는 언어(java·kotlin·typescript·python)
    public let version: String
    public init(word: String, slot: String, target: String, version: String) { self.word = word; self.slot = slot; self.target = target; self.version = version }
}

// 알려진 이름(설계 2장 표: Java·자바·Spring Boot·스프링·Kotlin·NestJS·네스트·FastAPI·React·리액트·Next.js·Vue·PostgreSQL·포스트그레스·MySQL 등).
// 긴 이름부터 본다(Spring Boot 를 Spring 보다 먼저). 한글 낱말은 뒤에 조사가 붙어도 찾고, 영문 낱말은 앞뒤가 영문 글자가 아닐 때만(JavaScript 의 Java 를 찾지 않게)
private let apTopicWords: [(String, String, String)] = [
    ("Spring Boot", "back", "spring"), ("SpringBoot", "back", "spring"), ("스프링 부트", "back", "spring"), ("스프링부트", "back", "spring"),
    ("스프링", "back", "spring"), ("Spring", "back", "spring"),
    ("NestJS", "back", "nest"), ("Nest.js", "back", "nest"), ("네스트JS", "back", "nest"), ("네스트", "back", "nest"),
    ("FastAPI", "back", "fastapi"), ("패스트API", "back", "fastapi"),
    ("Next.js", "front", "next"), ("NextJS", "front", "next"), ("넥스트JS", "front", "next"), ("넥스트", "front", "next"),
    ("React", "front", "react"), ("리액트", "front", "react"),
    ("Vue.js", "front", "vue"), ("Vue", "front", "vue"),
    ("PostgreSQL", "db", "postgresql"), ("Postgres", "db", "postgresql"), ("포스트그레스", "db", "postgresql"), ("포스트그레SQL", "db", "postgresql"),
    ("MySQL", "db", "mysql"), ("마이에스큐엘", "db", "mysql"),
    ("Kotlin", "lang", "kotlin"), ("코틀린", "lang", "kotlin"), ("Java", "lang", "java"), ("자바", "lang", "java"),
    ("TypeScript", "lang", "typescript"), ("타입스크립트", "lang", "typescript"), ("Python", "lang", "python"), ("파이썬", "lang", "python"),
]

private func isLatin(_ c: Character?) -> Bool { c.map { $0.isASCII && ($0.isLetter) } ?? false }

public func apTopicHits(_ text: String) -> [APTopicHit] {
    var hits: [APTopicHit] = []
    var taken: [Range<String.Index>] = []
    for (w, slot, target) in apTopicWords {
        var from = text.startIndex
        while let r = text.range(of: w, options: [.caseInsensitive], range: from..<text.endIndex) {
            from = r.upperBound
            if taken.contains(where: { $0.overlaps(r) }) { continue }
            let latin = w.unicodeScalars.first.map { $0.isASCII } ?? false
            if latin {
                let before = r.lowerBound > text.startIndex ? text[text.index(before: r.lowerBound)] : nil
                let after = r.upperBound < text.endIndex ? text[r.upperBound] : nil
                if isLatin(before) || isLatin(after) { continue }
            }
            taken.append(r)
            // 바로 뒤(빈칸 하나까지)의 버전 꼴: 21, 4.1, 4.1.1, v18
            let tail = String(text[r.upperBound...].prefix(16))
            let v = first(tail, "^\\s?v?([0-9]+(\\.[0-9]+){0,2})(?![0-9])") ?? ""
            hits.append(APTopicHit(word: String(text[r]), slot: slot, target: target, version: v))
        }
    }
    // 글에 나온 차례로
    return hits.sorted { a, b in
        (text.range(of: a.word)?.lowerBound ?? text.endIndex) < (text.range(of: b.word)?.lowerBound ?? text.endIndex)
    }
}

// 주제 글에서 칸 셋을 미리 채운다(origin "topic"). 판은 글에 있으면 그것, 없으면 빈 칸(보드가 추천 판을 넣는다).
// 백엔드: Spring 이면 언어 낱말이 Kotlin 일 때 Kotlin · Spring Boot, 아니면 Java · Spring Boot(언어 판은 Java·Kotlin 낱말 뒤의 판)
public func apTopicStack(_ text: String) -> APStackSet {
    let hits = apTopicHits(text)
    var set = APStackSet()
    let lang = hits.first { $0.slot == "lang" && ["java", "kotlin"].contains($0.target) }
    if let b = hits.first(where: { $0.slot == "back" }) {
        switch b.target {
        case "spring":
            let kotlin = lang?.target == "kotlin"
            set.back = APStackSlot(key: kotlin ? "spring-kotlin" : "spring-java", name: kotlin ? "Kotlin · Spring Boot" : "Java · Spring Boot",
                                   lang: kotlin ? "Kotlin" : "Java", langVersion: lang?.version ?? "", framework: "Spring Boot", frameworkVersion: b.version,
                                   build: "Gradle Groovy", origin: "topic")
        case "nest":
            set.back = APStackSlot(key: "nest", name: "TypeScript · NestJS", lang: "Node.js", framework: "NestJS", frameworkVersion: b.version, build: "npm", origin: "topic")
        default:
            let py = hits.first { $0.target == "python" }
            set.back = APStackSlot(key: "fastapi", name: "Python · FastAPI", lang: "Python", langVersion: py?.version ?? "", framework: "FastAPI",
                                   frameworkVersion: b.version, build: "pip", origin: "topic")
        }
    }
    if let f = hits.first(where: { $0.slot == "front" }) {
        let name = ["react": "React (Vite)", "next": "Next.js", "vue": "Vue"][f.target] ?? f.target
        set.front = APStackSlot(key: f.target, name: name, framework: ["react": "React", "next": "Next.js", "vue": "Vue"][f.target] ?? f.target,
                                frameworkVersion: f.version, build: f.target == "next" ? "npm" : "Vite", origin: "topic")
    }
    if let d = hits.first(where: { $0.slot == "db" }) {
        set.db = APStackSlot(key: d.target, name: d.target == "postgresql" ? "PostgreSQL" : "MySQL", framework: d.target == "postgresql" ? "PostgreSQL" : "MySQL",
                             frameworkVersion: d.version, origin: "topic")
    }
    return set
}

// 기존 저장소인데 주제 글이 다른 스택을 말할 때의 경고(설계 2장: 바꿀 수 없다, 스택 바꾸기는 그것을 주제로 한 따로 출발).
// 감지한 칸과 같은 칸의 다른 것만 알린다(감지하지 못한 칸은 알리지 않는다 — 그 칸을 더하는 일이 주제일 수 있다)
public func apStackConflicts(detected: APStackSet, topic text: String) -> [String] {
    var out: [String] = []
    for h in apTopicHits(text) {
        let slot: APStackSlot?
        switch h.slot {
        case "front": slot = detected.front
        case "back": slot = detected.back
        case "db": slot = detected.db
        default:
            // 언어 낱말: 감지한 백엔드의 언어와 다르면(Java ↔ Kotlin 처럼)
            if let b = detected.back, !b.lang.isEmpty, b.lang.lowercased() != h.target, ["java", "kotlin", "python", "typescript"].contains(h.target),
               !(h.target == "typescript" && b.lang == "Node.js") {
                let m = "주제의 '\(h.word)' 는 이 프로젝트 규격(\(b.lang))과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."
                if !out.contains(m) { out.append(m) }
            }
            continue
        }
        guard let s = slot else { continue }
        let same: Bool
        switch h.target {
        case "spring": same = s.framework == "Spring Boot"
        case "nest": same = s.framework == "NestJS"
        case "fastapi": same = s.framework == "FastAPI"
        case "react": same = s.framework == "React"
        case "next": same = s.framework == "Next.js"
        case "vue": same = s.framework == "Vue"
        case "postgresql": same = s.framework == "PostgreSQL"
        case "mysql": same = s.framework == "MySQL"
        default: same = true
        }
        if !same {
            let m = "주제의 '\(h.word)' 는 이 프로젝트 규격(\(s.framework.isEmpty ? s.name : s.framework))과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."
            if !out.contains(m) { out.append(m) }
        }
    }
    return out
}
