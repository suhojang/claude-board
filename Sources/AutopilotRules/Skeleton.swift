// 자율주행 규칙(AutopilotRules) — 주제로 시작의 뼈대 만들기와 판정 시나리오 커밋(5단계 ⑪-2, 2026-10-30, 설계 2장 "기술 스택"의 뼈대·"판정 시나리오 잠금").
// 새 폴더면 고른 스택의 공식 생성기로 뼈대를 만들어 자율주행 가지 autopilot/<실행 id> 의 첫 커밋으로 두고(모델 토큰 0), 초안 세션이 쓴 판정 시나리오를
// 그다음 커밋으로 둔다. 가지는 출발 전에 보드(앱 안 엔진의 seed 명령)가 출발 가지 머리에서 만들고, 출발 때 엔진이 그 가지를 그대로 이어받는다(run.json seed).
// 여기는 그 순수 규칙이다: 스택 → 생성기 계획(명령 글·필요한 도구·바깥 요청 여부·보드에 든 틀), 도구 확인, 계획을 돌리는 유스케이스(포트 APSkeletonWork),
// 커밋 글. 직접 입력한 스택은 생성기를 모르므로 뼈대 대신 첫 단위 "프로젝트 구성"을 모델이 한다(초안 세션 기계 검사가 본다).
// 사람이 읽는 글에는 긴 줄표를 쓰지 않는다
import Foundation

public enum APGenKind: String, Equatable, Sendable {
    case template     // 보드에 든 틀(파일 쓰기만, 바깥 요청 없음)
    case command      // 공식 생성기 명령(npm·npx 등, 패키지 레지스트리에 요청함)
    case download     // 공식 생성기 주소에서 zip 을 받아 풂(start.spring.io)
}

public struct APGenFile: Equatable, Sendable {
    public var path: String, text: String
    public init(_ path: String, _ text: String) { self.path = path; self.text = text }
}

public struct APGenTool: Equatable, Sendable {
    public var command: String      // 로그인 셸 PATH 에서 찾는 이름
    public var label: String        // 사람에게 보이는 이름
    public init(_ command: String, _ label: String) { self.command = command; self.label = label }
}

public struct APGenStep: Equatable, Sendable {
    public var slot: String         // back·front·db
    public var label: String        // 생성기 이름(쉬운 말)
    public var folder: String       // 저장소 뿌리 기준(빈 글자 = 뿌리)
    public var kind: APGenKind
    public var argv: [String] = []  // command: 폴더의 부모(뿌리)에서 돌릴 명령
    public var url = ""             // download: 받을 주소
    public var files: [APGenFile] = []   // template
    public var tools: [APGenTool] = []   // 생성(과 그 뒤 빌드)에 필요한 도구
    public var external: Bool { kind != .template }   // 바깥 요청(레지스트리·생성기 서버)을 하는가
    public init(slot: String, label: String, folder: String, kind: APGenKind) { self.slot = slot; self.label = label; self.folder = folder; self.kind = kind }
}

public struct APSkeletonPlan: Equatable, Sendable {
    public var steps: [APGenStep] = []
    public var firstUnit = false        // 직접 입력 칸이 있어 첫 단위 "프로젝트 구성"을 모델이 함
    public var notes: [String] = []     // 쉬운 말(준비 줄)
    public init() {}
}

// 프로젝트 이름 → 패키지 이름 조각(영문 소문자·숫자, 앞은 글자)
public func apPackagePart(_ name: String) -> String {
    let s = String(name.lowercased().filter { ($0 >= "a" && $0 <= "z") || ($0 >= "0" && $0 <= "9") })
    guard let f = s.first, f >= "a" && f <= "z" else { return "app" + s }
    return s
}

private func ver(_ s: APStackSlot, _ fallback: String) -> String { s.frameworkVersion.isEmpty ? fallback : s.frameworkVersion }
// React 칸의 빌드 도구 글 "Vite 8.3" 에서 판(없으면 latest)
private func viteVersion(_ s: APStackSlot) -> String {
    let p = s.build.split(separator: " ")
    return p.count == 2 && p[0] == "Vite" && p[1].range(of: "^[0-9][0-9.]*$", options: .regularExpression) != nil ? String(p[1]) : "latest"
}

// 스택 → 생성기 계획(설계 2장 "기술 스택"의 뼈대). 프론트와 백엔드를 함께 고르면 frontend/·backend/ 두 폴더, 하나면 뿌리.
// DB 는 로컬에서 띄우는 docker compose 틀(운영 DB 는 범위 밖). 직접 입력 칸은 생성기 없이 firstUnit
public func apSkeletonPlan(_ st: APStackSet, name: String) -> APSkeletonPlan {
    var p = APSkeletonPlan()
    let both = st.front != nil && st.back != nil
    let pkg = apPackagePart(name)
    if let b = st.back {
        let folder = both ? "backend" : ""
        switch b.key {
        case "fastapi":
            var s = APGenStep(slot: "back", label: "FastAPI 틀(보드에 든 것, 공식 생성기가 없음)", folder: folder, kind: .template)
            let py = b.langVersion.isEmpty ? "3.13" : b.langVersion
            s.files = apFastAPITemplate(name: name, python: py, fastapi: ver(b, "0.115"))
            s.tools = [APGenTool("python3", "Python(python3)")]
            p.steps.append(s)
        case "spring-java", "spring-kotlin":
            var s = APGenStep(slot: "back", label: "Spring Initializr(start.spring.io)", folder: folder, kind: .download)
            let type = ["Gradle Kotlin": "gradle-project-kotlin", "Maven": "maven-project"][b.build] ?? "gradle-project"
            let lang = b.key == "spring-kotlin" ? "kotlin" : "java"
            let java = b.key == "spring-kotlin" ? "" : b.langVersion
            var q = ["type=\(type)", "language=\(lang)", "bootVersion=\(ver(b, "4.1.1"))", "groupId=com.example", "artifactId=\(pkg)", "name=\(pkg)",
                     "packageName=com.example.\(pkg)", "dependencies=web"]
            if !java.isEmpty { q.insert("javaVersion=\(java)", at: 3) }
            s.url = "https://start.spring.io/starter.zip?" + q.joined(separator: "&")
            s.tools = [APGenTool("unzip", "unzip"), APGenTool("java", "JDK(java)")]
            p.steps.append(s)
        case "nest":
            var s = APGenStep(slot: "back", label: "Nest CLI(nest new)", folder: folder, kind: .command)
            s.argv = ["npx", "--yes", "@nestjs/cli@\(ver(b, "latest"))", "new", folder.isEmpty ? pkg : folder, "--package-manager", "npm", "--skip-git"]
            if folder.isEmpty { s.argv[4] = "." }
            s.tools = [APGenTool("node", "Node.js(node)"), APGenTool("npx", "npm(npx)")]
            p.steps.append(s)
        default:
            p.firstUnit = true
            p.notes.append("백엔드 \(b.name) 는 보드가 생성기를 모르는 스택이라 첫 단위 \"\(apProjectSetupTitle)\"를 모델이 합니다")
        }
    }
    if let f = st.front {
        let folder = both ? "frontend" : ""
        let dir = folder.isEmpty ? "." : folder
        var s = APGenStep(slot: "front", label: "", folder: folder, kind: .command)
        s.tools = [APGenTool("node", "Node.js(node)"), APGenTool("npm", "npm")]
        switch f.key {
        case "react":
            s.label = "Vite(npm create vite, react-ts)"
            s.argv = ["npm", "create", "--yes", "vite@\(viteVersion(f))", dir, "--", "--template", "react-ts"]
            p.steps.append(s)
        case "next":
            s.label = "create-next-app"
            s.argv = ["npx", "--yes", "create-next-app@\(ver(f, "latest"))", dir, "--ts", "--app", "--use-npm", "--yes", "--disable-git"]
            s.tools = [APGenTool("node", "Node.js(node)"), APGenTool("npx", "npm(npx)")]
            p.steps.append(s)
        case "vue":
            s.label = "create-vue(npm create vue)"
            s.argv = ["npm", "create", "--yes", "vue@latest", dir, "--", "--ts", "--default"]   // create-vue 는 Vue 판과 따로 나가 latest
            p.steps.append(s)
        default:
            p.firstUnit = true
            p.notes.append("프론트 \(f.name) 는 보드가 생성기를 모르는 스택이라 첫 단위 \"\(apProjectSetupTitle)\"를 모델이 합니다")
        }
    }
    if let d = st.db {
        switch d.key {
        case "postgresql", "mysql":
            var s = APGenStep(slot: "db", label: "docker compose 틀(로컬 시험 DB)", folder: "", kind: .template)
            s.files = [apComposeTemplate(d)]
            p.steps.append(s)
        default:
            p.firstUnit = true
            p.notes.append("DB \(d.name) 는 보드가 틀을 모르는 것이라 첫 단위 \"\(apProjectSetupTitle)\"에서 모델이 구성합니다")
        }
    }
    if p.steps.isEmpty, !p.firstUnit { p.notes.append("고른 스택이 없어 뼈대를 만들지 않습니다") }
    return p
}

// FastAPI 틀 — 공식 생성기가 없어 보드가 든다(설계 2장). 의존성 설치는 하지 않는다(그 위 구성은 모델이 단위로)
public func apFastAPITemplate(name: String, python: String, fastapi: String) -> [APGenFile] {
    let pkg = apPackagePart(name)
    return [
        APGenFile("pyproject.toml", """
        [project]
        name = "\(pkg)"
        version = "0.1.0"
        description = "Claude Board 자율주행이 만든 FastAPI 뼈대"
        requires-python = ">=\(python)"
        dependencies = ["fastapi>=\(fastapi)", "uvicorn>=0.30"]

        """),
        APGenFile("app/__init__.py", ""),
        APGenFile("app/main.py", """
        from fastapi import FastAPI

        app = FastAPI(title="\(pkg)")


        @app.get("/health")
        def health() -> dict:
            return {"status": "ok"}

        """),
        APGenFile(".gitignore", "__pycache__/\n*.pyc\n.venv/\n.pytest_cache/\n"),
        APGenFile("README.md", """
        # \(name)

        Claude Board 자율주행이 주제로 시작에서 만든 FastAPI 뼈대입니다(보드에 든 틀, 바깥 요청 없음).
        띄우기: `python3 -m venv .venv && .venv/bin/pip install -e . && .venv/bin/uvicorn app.main:app`

        """),
    ]
}

public func apComposeTemplate(_ d: APStackSlot) -> APGenFile {
    let v = d.frameworkVersion.isEmpty ? d.langVersion : d.frameworkVersion
    if d.key == "mysql" {
        return APGenFile("compose.yaml", """
        # 로컬 시험 DB(Claude Board 자율주행 뼈대). 운영 DB 연결은 범위 밖입니다
        services:
          db:
            image: mysql:\(v.isEmpty ? "8.4" : v)
            environment:
              MYSQL_DATABASE: app
              MYSQL_USER: app
              MYSQL_PASSWORD: app-local-only
              MYSQL_ROOT_PASSWORD: app-local-only
            ports:
              - "127.0.0.1:3306:3306"

        """)
    }
    return APGenFile("compose.yaml", """
    # 로컬 시험 DB(Claude Board 자율주행 뼈대). 운영 DB 연결은 범위 밖입니다
    services:
      db:
        image: postgres:\(v.isEmpty ? "18" : v)
        environment:
          POSTGRES_DB: app
          POSTGRES_USER: app
          POSTGRES_PASSWORD: app-local-only
        ports:
          - "127.0.0.1:5432:5432"

    """)
}

// 생성기 명령 글(준비 줄·시험) — 사람이 볼 한 줄
public func apGenCommandText(_ s: APGenStep) -> String {
    switch s.kind {
    case .template: return "보드에 든 틀: " + s.files.map { (s.folder.isEmpty ? "" : s.folder + "/") + $0.path }.joined(separator: ", ")
    case .command: return s.argv.map { $0.contains(" ") ? "'\($0)'" : $0 }.joined(separator: " ")
    case .download: return "GET \(s.url) → \(s.folder.isEmpty ? "." : s.folder)/ 에 풂"
    }
}

// 도구 확인(포트 APToolProbe 의 command) — 없으면 준비 줄 실패와 까닭(보드는 도구를 깔지 않는다)
public func apSkeletonToolProblems(_ p: APSkeletonPlan, probe: APToolProbe) -> [String] {
    var out: [String] = []
    for s in p.steps {
        let missing = s.tools.filter { probe.command($0.command) == nil }
        guard !missing.isEmpty else { continue }
        out.append("\(s.label): \(missing.map(\.label).joined(separator: "·")) 를 찾지 못했습니다. 보드는 도구를 깔지 않습니다. 깐 뒤 준비를 다시 하거나 다른 스택을 고르세요")
    }
    return out
}

public struct APSkeletonResult: Equatable, Sendable {
    public var ok = false
    public var done: [String] = []        // 한 일(쉬운 말)
    public var problems: [String] = []
    public var commit: String?
    public var external = 0               // 바깥 요청을 하는 걸음 수(돌린 것)
    public init() {}
}

public func apSkeletonMessage(_ p: APSkeletonPlan) -> String {
    "뼈대: " + p.steps.map(\.label).joined(separator: " · ") + " (Claude Board 자율주행이 출발 전에 만듦, 모델 0)"
}

// 유스케이스: 뼈대 만들기 — 도구 확인 → 걸음마다(틀 쓰기·명령·받아 풀기) → 커밋 하나. 하다 실패하면 거기서 멈춘다(임시 작업 트리는 엔진이 치움).
// allowExternal 이 거짓이면 바깥 요청 걸음을 돌리지 않고 실패로 둔다(시험: 레지스트리를 두드리지 않음)
public func apMakeSkeleton(_ p: APSkeletonPlan, probe: APToolProbe, work: APSkeletonWork, allowExternal: Bool = true) -> APSkeletonResult {
    var r = APSkeletonResult()
    r.problems = apSkeletonToolProblems(p, probe: probe)
    guard r.problems.isEmpty else { return r }
    for s in p.steps {
        if s.external && !allowExternal {
            r.problems.append("\(s.label): 바깥 요청을 하는 생성기라 돌리지 않았습니다(\(apGenCommandText(s)))"); return r
        }
        switch s.kind {
        case .template:
            for f in s.files {
                let path = (s.folder.isEmpty ? "" : s.folder + "/") + f.path
                guard work.write(path, f.text) else { r.problems.append("\(s.label): \(path) 를 쓰지 못했습니다"); return r }
            }
            r.done.append("\(s.label): 파일 \(s.files.count)개")
        case .command:
            let x = work.run(s.argv, in: "")
            r.external += 1
            guard x.code == 0 else { r.problems.append("\(s.label) 이 실패했습니다(종료 코드 \(x.code)): " + String(x.out.suffix(300))); return r }
            r.done.append("\(s.label): \(apGenCommandText(s))")
        case .download:
            r.external += 1
            if let why = work.download(s.url, into: s.folder) { r.problems.append("\(s.label): \(why)"); return r }
            r.done.append("\(s.label): 받아 풂")
        }
    }
    guard !p.steps.isEmpty else { r.ok = true; return r }
    let c = work.commit(apSkeletonMessage(p))
    if let e = c.error { r.problems.append("뼈대를 커밋하지 못했습니다: \(e)"); return r }
    r.commit = c.sha
    r.ok = true
    return r
}

public func apScenarioCommitMessage(units: Int, scenarios: Int) -> String {
    "판정 시나리오: 단위 \(units)개 · 시나리오 \(scenarios)개 (초안 세션이 씀, 출발 뒤에는 고정본으로 판정)"
}

// 출발 전 자율주행 가지(run.json seed) — 엔진 출발 전 확인·출발·판정 미리 돌리기가 이 기록으로 그 가지를 이어받는다
public struct APSeed: Equatable, Sendable {
    public var repo: String
    public var branch: String        // autopilot/<실행 id>
    public var base: String          // 출발 가지 이름
    public var baseHead: String      // 가지를 만든 때 출발 가지 머리
    public var head: String          // 마지막 커밋(뼈대·시나리오)
    public init(repo: String, branch: String, base: String, baseHead: String, head: String) {
        self.repo = repo; self.branch = branch; self.base = base; self.baseHead = baseHead; self.head = head
    }
}
public func apSeedParse(_ v: Any?) -> APSeed? {
    guard let o = v as? [String: Any], let r = o["repo"] as? String, let b = o["branch"] as? String, let base = o["base"] as? String,
          let bh = o["base_head"] as? String, let h = o["head"] as? String, !h.isEmpty else { return nil }
    return APSeed(repo: r, branch: b, base: base, baseHead: bh, head: h)
}

// 출발·출발 전 확인에서 이어받아도 되나 — 가지 이름이 이 실행 것, 그 가지 머리가 기록과 같고, 출발 가지 머리가 그대로이고, 출발 가지 머리가 가지의 조상.
// 아니면 까닭(출발하지 않음. 보드에서 준비를 다시 하면 새로 만든다)
public func apSeedProblem(_ s: APSeed, runId: String, branchHead: String?, baseHeadNow: String?, isAncestor: Bool) -> String? {
    if s.branch != "autopilot/" + runId { return "출발 전 자율주행 가지 이름(\(s.branch))이 이 실행(autopilot/\(runId))과 다릅니다" }
    guard let bh = branchHead else { return "출발 전 자율주행 가지 \(s.branch) 가 없습니다(준비를 다시 하세요)" }
    if bh != s.head { return "출발 전 자율주행 가지 \(s.branch) 가 준비 뒤에 바뀌었습니다(준비가 커밋한 \(s.head.prefix(7)), 지금 \(bh.prefix(7)))" }
    guard let now = baseHeadNow else { return "출발 가지 \(s.base) 가 없습니다" }
    if now != s.baseHead { return "출발 가지 \(s.base) 가 준비 뒤에 움직였습니다(준비 때 \(s.baseHead.prefix(7)), 지금 \(now.prefix(7))). 준비를 다시 하세요" }
    if !isAncestor { return "출발 전 자율주행 가지가 출발 가지 \(s.base) 에서 시작하지 않았습니다" }
    return nil
}
