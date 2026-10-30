// 주제로 시작의 기술 스택 규칙(Sources/AutopilotRules/Stack.swift)과 새 폴더 만들기(NewRepo.swift) 시험 — 2026-10-30 2단계 5-2b 에서 새로 씀.
// 낱말 대조·감지·경고는 갈래마다 줄을 두고, 새 폴더는 가짜 폴더·git 어댑터로 차례와 멈추는 자리를 본다(예시는 가상: room-booking·todo·shop)
import Foundation
import Testing
import AutopilotRules

// MARK: - 주제 글 낱말 대조

struct TopicRow: Row {
    let note: String
    let text: String
    let want: String     // "front|back|db" 칸마다 "키 판"(없으면 -), 언어 판은 back 뒤 "/언어 판"
}
private func show(_ s: APStackSet) -> String {
    func one(_ x: APStackSlot?) -> String { x.map { "\($0.key.isEmpty ? $0.name : $0.key) \($0.frameworkVersion)".trimmingCharacters(in: .whitespaces) } ?? "-" }
    let lang = s.back.map { $0.langVersion.isEmpty ? "" : "/\($0.lang) \($0.langVersion)" } ?? ""
    return "\(one(s.front))|\(one(s.back))\(lang)|\(one(s.db))"
}

private let topicRows: [TopicRow] = [
    .init(note: "와이어프레임 예시: Spring Boot · PostgreSQL, 버전 없음", text: "회의실 예약 API 를 Spring Boot 로 만든다. 예약·취소·겹침 막기, 관리자 승인. DB 는 PostgreSQL .",
          want: "-|spring-java|postgresql"),
    .init(note: "스택이 없는 주제", text: "회의실 예약 API 를 만든다. 예약·취소·겹침 막기, 관리자 승인.", want: "-|-|-"),
    .init(note: "한글 이름과 조사·버전", text: "스프링부트 4.1 과 자바21로, 포스트그레스 17 에 붙인다", want: "-|spring-java 4.1/Java 21|postgresql 17"),
    .init(note: "Kotlin 이 있으면 Kotlin · Spring Boot", text: "코틀린 2.4 와 Spring 으로 만든다", want: "-|spring-kotlin/Kotlin 2.4|-"),
    .init(note: "NestJS 와 React(앞은 Vite 꼴)", text: "NestJS 12 백엔드와 리액트 19.3 화면, MySQL 8.4", want: "react 19.3|nest 12|mysql 8.4"),
    .init(note: "Next.js 는 React 보다 먼저 본다", text: "Next.js 16 으로 React 화면", want: "next 16|-|-"),
    .init(note: "FastAPI 는 Python 판과 함께", text: "Python 3.14 와 FastAPI 0.162 로 만든다", want: "-|fastapi 0.162/Python 3.14|-"),
    .init(note: "JavaScript 의 Java 는 찾지 않는다(영문 낱말은 앞뒤가 영문이 아닐 때만)", text: "JavaScript 로 만든 Reactive 화면", want: "-|-|-"),
    .init(note: "Vue.js 와 v 붙은 판", text: "Vue.js v3.5 화면과 Postgres", want: "vue 3.5|-|postgresql"),
    .init(note: "대소문자를 가리지 않는다", text: "spring boot 와 postgresql", want: "-|spring-java|postgresql"),
]

@Suite("자율주행 주제로 시작: 주제 글 낱말 대조") struct TopicWordTests {
    @Test("칸 셋을 미리 채운다", arguments: topicRows)
    func 표(_ r: TopicRow) { #expect(show(apTopicStack(r.text)) == r.want) }

    @Test("찾은 낱말은 적힌 그대로, 글에 나온 차례로")
    func 낱말() {
        let h = apTopicHits("DB 는 포스트그레스 17, 백엔드는 스프링 부트로")
        #expect(h.map(\.word) == ["포스트그레스", "스프링 부트"])
        #expect(h.first?.version == "17")
    }
}

// MARK: - 기존 저장소 감지

struct DetectRow: Row {
    let note: String
    let files: [String: String]
    let want: String
    var build = ""
    var sources: [String] = []
}

private let gradleSpring = """
plugins {
    id 'java'
    id 'org.springframework.boot' version '4.0.8'
    id 'io.spring.dependency-management' version '1.1.7'
}
java { toolchain { languageVersion = JavaLanguageVersion.of(21) } }
"""
private let ktsSpring = """
plugins {
    kotlin("jvm") version "2.4.0"
    kotlin("plugin.spring") version "2.4.0"
    id("org.springframework.boot") version "4.1.1"
}
kotlin { jvmToolchain(25) }
"""
private let pomSpring = """
<project><parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId>
<version>3.5.2</version></parent><properties><java.version>17</java.version></properties></project>
"""

private let detectRows: [DetectRow] = [
    .init(note: "아무 파일도 없음 → 감지된 것 없음", files: [:], want: "-|-|-"),
    .init(note: "Gradle Groovy Spring Boot + application.yml 의 postgresql + compose 이미지 판", files: [
        "build.gradle": gradleSpring, "src/main/resources/application.yml": "spring:\n  datasource:\n    url: jdbc:postgresql://localhost:5432/todo\n",
        "docker-compose.yml": "services:\n  db:\n    image: postgres:17\n"], want: "-|spring-java 4.0.8/Java 21|postgresql 17", build: "Gradle Groovy",
          sources: ["build.gradle", "src/main/resources/application.yml"]),
    .init(note: "Gradle Kotlin(kts) + Kotlin 플러그인 → Kotlin · Spring Boot", files: ["build.gradle.kts": ktsSpring], want: "-|spring-kotlin 4.1.1/Kotlin 25|-",
          build: "Gradle Kotlin", sources: ["build.gradle.kts"]),
    .init(note: "Maven 부모 판과 java.version", files: ["pom.xml": pomSpring], want: "-|spring-java 3.5.2/Java 17|-", build: "Maven", sources: ["pom.xml"]),
    .init(note: "NestJS(engines.node) + pg", files: ["package.json": #"{"dependencies":{"@nestjs/core":"^11.1.0","pg":"^8"},"engines":{"node":">=22"}}"#],
          want: "-|nest 11.1.0/Node.js 22|postgresql", build: "npm", sources: ["package.json"]),
    .init(note: "뿌리 package.json 에 React+Vite, 백엔드 폴더에 FastAPI(pyproject requires-python·psycopg)", files: [
        "package.json": #"{"dependencies":{"react":"^19.3.0"},"devDependencies":{"vite":"^8.3.0"}}"#,
        "backend/pyproject.toml": "[project]\nrequires-python = \">=3.13\"\ndependencies = [\"fastapi[standard]>=0.160.0\", \"psycopg[binary]\"]\n"],
          want: "react 19.3.0|fastapi 0.160.0/Python 3.13|postgresql", build: "pip-or-pyproject", sources: []),
    .init(note: "frontend 폴더의 Next.js 와 backend 폴더의 Spring", files: ["frontend/package.json": #"{"dependencies":{"next":"16.4.1","react":"19.3.1"}}"#,
                                                                         "backend/build.gradle": gradleSpring], want: "next 16.4.1|spring-java 4.0.8/Java 21|-", build: "Gradle Groovy",
          sources: ["backend/build.gradle"]),
    .init(note: "prisma provider mysql", files: ["package.json": #"{"dependencies":{"express":"^5.1.0"}}"#, "prisma/schema.prisma": "datasource db {\n  provider = \"mysql\"\n}\n"],
          want: "-|Node.js · Express 5.1.0|mysql", build: "npm", sources: ["package.json"]),
    .init(note: "Go 모듈과 Gin", files: ["go.mod": "module example.com/todo\n\ngo 1.24\n\nrequire github.com/gin-gonic/gin v1.10.0\n"], want: "-|Go · Gin/Go 1.24|-", build: "go", sources: ["go.mod"]),
    .init(note: "react-native 는 웹 프론트로 보지 않는다", files: ["package.json": #"{"dependencies":{"react":"19.0.0","react-native":"0.80.0"}}"#], want: "-|-|-"),
]

@Suite("자율주행 주제로 시작: 기존 저장소 기술 스택 감지") struct DetectTests {
    @Test("빌드·설정 파일에서 감지", arguments: detectRows)
    func 표(_ r: DetectRow) {
        let s = apDetectStack(r.files)
        #expect(show(s) == r.want)
        if !r.build.isEmpty, r.build != "pip-or-pyproject" { #expect(s.back?.build == r.build) }
        let found: [String] = (s.back?.sources ?? []) + (s.db?.sources ?? [])
        if !r.sources.isEmpty { #expect(Array(found.prefix(r.sources.count)) == r.sources) }
        for slot in [s.front, s.back, s.db].compactMap({ $0 }) { #expect(slot.origin == "detected") }
    }

    // 가짜 파일 어댑터 — 규칙이 고른 자리만 읽는지 센다
    private final class FakeFiles: APStackFiles, @unchecked Sendable {
        var asked: [String] = []
        let files: [String: String]
        init(_ f: [String: String]) { files = f }
        func read(_ root: String, _ relative: String) -> String? { asked.append(root + "|" + relative); return files[relative] }
    }

    @Test("유스케이스는 고른 상대 경로만 그 뿌리에서 읽는다")
    func 읽는자리() {
        let f = FakeFiles(["build.gradle": gradleSpring, "secret.env": "X=1"])
        let s = apDetectStack(root: "/w/todo", files: f)
        #expect(s.back?.key == "spring-java")
        #expect(f.asked.count == apStackCandidates.count)
        #expect(f.asked.allSatisfy { $0.hasPrefix("/w/todo|") && !$0.contains("secret") && !$0.contains("..") })
    }
}

// MARK: - 기존 저장소인데 주제가 다른 스택

struct ConflictRow: Row {
    let note: String
    let detected: [String: String]
    let topic: String
    let want: [String]
}
private let conflictRows: [ConflictRow] = [
    .init(note: "와이어프레임 예시: Spring Boot 저장소에 NestJS 주제", detected: ["build.gradle": gradleSpring], topic: "회의실 예약 기능을 NestJS 로 더한다.",
          want: ["주제에 적힌 'NestJS': 이 프로젝트 규격(Spring Boot)과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."]),
    .init(note: "같은 스택이면 경고 없음", detected: ["build.gradle": gradleSpring], topic: "스프링 부트로 예약 API 를 더한다", want: []),
    .init(note: "감지하지 못한 칸(프론트)은 알리지 않는다", detected: ["build.gradle": gradleSpring], topic: "React 화면을 더한다", want: []),
    .init(note: "언어가 다르면(Java 저장소에 Kotlin)", detected: ["build.gradle": gradleSpring], topic: "Kotlin 으로 다시 쓴다",
          want: ["주제에 적힌 'Kotlin': 이 프로젝트 규격(Java)과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."]),
    .init(note: "DB 가 다르면", detected: ["build.gradle": gradleSpring, "src/main/resources/application.yml": "url: jdbc:postgresql://x"], topic: "MySQL 에 붙인다",
          want: ["주제에 적힌 'MySQL': 이 프로젝트 규격(PostgreSQL)과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."]),
    .init(note: "같은 낱말이 두 번이면 한 번만", detected: ["build.gradle": gradleSpring], topic: "NestJS 로, 다시 말해 NestJS 로", want: [
        "주제에 적힌 'NestJS': 이 프로젝트 규격(Spring Boot)과 달라 쓰지 않습니다. 스택을 바꾸려면 그 일을 주제로 따로 출발합니다."]),
]

@Suite("자율주행 주제로 시작: 기존 저장소의 다른 스택 경고") struct ConflictTests {
    @Test("감지한 칸과 다른 낱말만", arguments: conflictRows)
    func 표(_ r: ConflictRow) { #expect(apStackConflicts(detected: apDetectStack(r.detected), topic: r.topic) == r.want) }
}

// MARK: - 새 폴더 만들기

struct NewRepoRow: Row {
    let note: String
    let parent: String
    let name: String
    var parentState: APPathState = .dir
    var targetState: APPathState = .missing
    let want: String     // 계획 경로 또는 첫 고칠 것
}
private let newRepoRows: [NewRepoRow] = [
    .init(note: "보통: ~/workspace 아래 새 이름", parent: "/Users/u/workspace", name: "room-booking", want: "/Users/u/workspace/room-booking"),
    .init(note: "끝의 / 는 뗀다", parent: "/Users/u/workspace/", name: "todo", want: "/Users/u/workspace/todo"),
    .init(note: "빈 폴더는 쓴다", parent: "/Users/u/workspace", name: "todo", targetState: .emptyDir, want: "/Users/u/workspace/todo"),
    .init(note: "무엇이 든 폴더에는 만들지 않는다", parent: "/Users/u/workspace", name: "todo", targetState: .dir,
          want: "그 자리에 이미 무엇이 든 폴더가 있습니다. 기존 저장소로 고르거나 다른 이름을 씁니다"),
    .init(note: "이름이 비었다", parent: "/Users/u/workspace", name: " ", want: "새 폴더 이름을 넣어 주세요"),
    .init(note: "이름에 빈칸·한글", parent: "/Users/u/workspace", name: "회의실 예약",
          want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)"),
    .init(note: "점으로 시작하는 이름", parent: "/Users/u/workspace", name: ".hidden",
          want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)"),
    .init(note: "이름에 / (다른 자리로 빠지기)", parent: "/Users/u/workspace", name: "a/../../x",
          want: "새 폴더 이름은 영문·숫자·.-_ 로 쓰고 영문이나 숫자로 시작합니다(엔진이 프로젝트 이름과 작업 트리 경로에 씁니다)"),
    .init(note: "홈 밖", parent: "/tmp", name: "todo", want: "위치는 홈 폴더 아래여야 합니다"),
    .init(note: "앱 자리 안", parent: "/Users/u/Library/Application Support", name: "todo",
          want: "위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다"),
    .init(note: "엔진 작업 트리 자리 안", parent: "/Users/u/.claude-board/worktrees", name: "todo",
          want: "위치가 앱·Claude 자리(~/Library, ~/.claude, ~/.claude-board) 안입니다. 작업 폴더 자리(예: ~/workspace)를 고릅니다"),
    .init(note: "위치에 ..", parent: "/Users/u/workspace/../Library", name: "todo", want: "위치에 . 이나 .. 을 쓰지 않습니다"),
    .init(note: "위치 폴더가 없다", parent: "/Users/u/nowhere", name: "todo", parentState: .missing, want: "위치 폴더가 없습니다"),
    .init(note: "상대 경로 위치", parent: "workspace", name: "todo", want: "위치는 / 로 시작하는 경로로 씁니다"),
    .init(note: "홈 바로 아래도 된다", parent: "/Users/u", name: "todo", want: "/Users/u/todo"),
    .init(note: "비슷한 이름의 다른 홈은 홈 밖", parent: "/Users/u2/workspace", name: "todo", want: "위치는 홈 폴더 아래여야 합니다"),
]

@Suite("자율주행 주제로 시작: 새 폴더") struct NewRepoTests {
    @Test("이름과 자리 확인", arguments: newRepoRows)
    func 표(_ r: NewRepoRow) {
        let c = apNewRepoCheck(parent: r.parent, name: r.name, home: "/Users/u", parentState: r.parentState, targetState: r.targetState)
        #expect((c.plan?.path ?? c.problems.first ?? "?") == r.want)
        if let p = c.plan { #expect(p.branches == ["dev", "live", "master"] && p.head == "dev" && p.base == "live") }
    }

    @Test("git 차례: init(처음 가지 dev) → 빈 첫 커밋(사람 이름·서명 끔·hooks 건너뜀) → 나머지 가지")
    func 차례() {
        let p = apNewRepoCheck(parent: "/Users/u/workspace", name: "todo", home: "/Users/u", parentState: .dir, targetState: .missing).plan!
        let s = apNewRepoSteps(p, identity: ("김 개발", "dev@example.com"))
        #expect(s.count == 4)
        #expect(s[0] == ["init", "-q", "-b", "dev"])
        #expect(s[1].contains("user.name=김 개발") && s[1].contains("commit.gpgsign=false") && s[1].contains("--allow-empty") && s[1].contains("--no-verify"))
        #expect(s[2] == ["branch", "live"] && s[3] == ["branch", "master"])
        #expect(apNewRepoSteps(p, identity: nil)[1].contains("user.name=Claude Board"))
    }

    private final class FakeFolders: APFolders, @unchecked Sendable {
        var st: [String: APPathState]
        var made: [String] = []
        var canMake = true
        init(_ s: [String: APPathState]) { st = s }
        func state(_ path: String) -> APPathState { st[path] ?? .missing }
        func makeDir(_ path: String) -> Bool { guard canMake else { return false }; made.append(path); st[path] = .emptyDir; return true }
    }
    private final class FakeGit: APGitRunner, @unchecked Sendable {
        var ran: [[String]] = []
        var failAt: String?
        var head = "c0ffee1234567890"
        var branches: Set<String> = []
        func run(_ args: [String], in dir: String) -> (code: Int32, out: String) {
            ran.append(args)
            let verb = args.first { !$0.hasPrefix("-") && !$0.contains("=") } ?? ""
            if verb == failAt { return (128, "fatal: 가짜 실패") }
            switch verb {
            case "init": branches = []
            case "commit": branches.insert("dev")
            case "branch": branches.insert(args.last ?? "")
            case "rev-parse": return branches.contains(String((args.last ?? "").dropFirst(11))) ? (0, head) : (1, "")
            default: break
            }
            return (0, "")
        }
        func identity() -> (name: String, email: String)? { nil }
    }

    @Test("유스케이스: 없는 자리면 폴더를 만들고 git 넷, 가지 셋이 같은 커밋")
    func 만들기() {
        let plan = apNewRepoCheck(parent: "/Users/u/workspace", name: "todo", home: "/Users/u", parentState: .dir, targetState: .missing).plan!
        let f = FakeFolders([:]), g = FakeGit()
        let r = apCreateRepo(plan, folders: f, git: g)
        #expect(r.ok && r.made && r.commit == "c0ffee1234567890" && r.error == nil)
        #expect(f.made == ["/Users/u/workspace/todo"])
        #expect(g.ran.filter { $0.first != "rev-parse" }.count == 4)
    }

    @Test("유스케이스: 하다 실패하면 거기서 멈추고 만든 것을 지우지 않는다")
    func 실패() {
        let plan = apNewRepoCheck(parent: "/Users/u/workspace", name: "todo", home: "/Users/u", parentState: .dir, targetState: .missing).plan!
        let f = FakeFolders([:]), g = FakeGit()
        g.failAt = "commit"
        let r = apCreateRepo(plan, folders: f, git: g)
        #expect(!r.ok && r.made)
        #expect(r.error == "git commit 이 실패했습니다(종료 코드 128): fatal: 가짜 실패")
        #expect(!g.ran.contains { $0.first == "branch" })
        // 그사이 누가 그 자리에 파일을 두었으면 만들지 않는다
        let f2 = FakeFolders(["/Users/u/workspace/todo": .dir]), g2 = FakeGit()
        let r2 = apCreateRepo(plan, folders: f2, git: g2)
        #expect(!r2.ok && g2.ran.isEmpty && f2.made.isEmpty)
        // 폴더를 만들지 못함
        let f3 = FakeFolders([:]); f3.canMake = false
        #expect(apCreateRepo(plan, folders: f3, git: FakeGit()).error == "폴더를 만들지 못했습니다: /Users/u/workspace/todo")
    }
}

// MARK: - 자료 링크

@Suite("자율주행 주제로 시작: 자료 링크 줄") struct LinkTests {
    @Test("종류와 읽기 상태(연결 전이라 읽지 않는다)", arguments: [
        ("https://www.figma.com/design/Rm8k2Tq/room?node-id=12-4", "Figma needs_connection"),
        ("  https://example.atlassian.net/wiki/spaces/OPS/pages/48213  ", "Confluence needs_connection"),
        ("https://example.atlassian.net/browse/SHOP-12", "Jira needs_connection"),
        ("https://www.notion.so/team/abc123", "Notion needs_connection"),
        ("https://team.notion.site/page", "Notion needs_connection"),
        ("https://docs.example.com/policy", "웹 needs_connection"),
        ("http://intranet.example.com/a", "웹 needs_connection"),
        ("figma.com/design/x", "웹 unreadable"),
        ("ftp://files.example.com/a", "웹 unreadable"),
        ("https://localhost/a", "웹 unreadable"),
        ("회의실 정책 문서", "웹 unreadable"),
        ("https://u:secret@example.com/a", "웹 unreadable"),
    ])
    func 표(_ a: (String, String)) {
        let l = apLinkInfo(a.0)
        #expect("\(l.kind) \(l.state)" == a.1)
        #expect(!l.url.contains("secret"))
    }

    @Test("더하기: 같은 주소 한 번, 빈 주소, 20줄까지")
    func 더하기() {
        let a = apLinkInfo("https://www.figma.com/design/x")
        #expect(apLinkAddProblem(a, to: []) == nil)
        #expect(apLinkAddProblem(a, to: [a]) == "이미 있는 주소입니다")
        #expect(apLinkAddProblem(apLinkInfo(""), to: []) == "주소를 붙여 넣어 주세요")
        let many = (0..<20).map { apLinkInfo("https://example.com/\($0)") }
        #expect(apLinkAddProblem(apLinkInfo("https://example.com/x"), to: many) == "자료 링크는 20줄까지입니다")
    }
}
