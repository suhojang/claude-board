// 보드 규칙 시험 — Swift Testing 으로 돈다(Command Line Tools 에는 XCTest 가 없다).
// CLT 만 있는 Mac: swift test --disable-keychain --triple "$(uname -m)-apple-macosx14.0" --toolset clt-testing.json
// 입력·기대값은 되도록 표(매개변수 시험)로 쓴다 — 나중에 Windows 판과 함께 쓸 JSON 표로 옮기기 쉽게. 기대값은 지금 출력이 아니라 의도한 동작이다
import Foundation
import Testing
import BoardRules

private let t0 = Date(timeIntervalSince1970: 1_000_000)

private func session(_ id: String, dir: String = "/p", state: String = "idle",
                     since: TimeInterval = 0, updated: TimeInterval = 0, message: String = "", topic: String = "") -> Session {
    Session(id: id, cwd: dir, projectDir: dir, state: state, since: t0 + since, updated: t0 + updated, headless: false,
            tty: "", topic: topic, message: message, resumeCmd: "", file: "", ctx: nil, brain: nil, pid: 0, subagents: [],
            shells: 0, paused: false, muted: false, limitAt: nil, name: nil)
}

private func record(_ json: String) throws -> [String: Any] {
    try #require(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
}

// MARK: - 홈 경로 ~ 치환 (고친 버그 a)

@Suite("화면 경로의 홈 줄이기") struct DisplayPathTests {
    @Test("홈과 같거나 홈 + / 로 시작할 때만 앞부분을 ~ 로", arguments: [
        ("/Users/me", "/Users/me", "~"),
        ("/Users/me/work", "/Users/me", "~/work"),
        ("/Users/me/", "/Users/me", "~/"),
        ("/Users/me/한글 폴더/x", "/Users/me", "~/한글 폴더/x"),
        ("/Users/me/a/Users/me/b", "/Users/me", "~/a/Users/me/b"),    // 뒤에 다시 나온 홈은 그대로(예전: ~/a~/b)
        ("/Users/meow/x", "/Users/me", "/Users/meow/x"),              // 홈 이름으로 시작하는 다른 폴더(예전: ~ow/x)
        ("/Users/meow", "/Users/me", "/Users/meow"),
        ("/data/Users/me/x", "/Users/me", "/data/Users/me/x"),        // 경로 중간의 홈(예전: /data~/x)
        ("/Users", "/Users/me", "/Users"),
        ("?", "/Users/me", "?"),                                       // cwd 를 모르는 세션
        ("/Users/me/x", "", "/Users/me/x"),                            // 홈을 모름
        ("/x", "/", "/x"),
    ])
    func 표(path: String, home: String, expected: String) {
        #expect(displayPath(path, home: home) == expected)
    }

    @Test("카드 경로(Group.path)도 같은 규칙") func 카드_경로() {
        let home = NSHomeDirectory()
        #expect(Group(id: home, sessions: []).path == "~")
        #expect(Group(id: home + "/w", sessions: []).path == "~/w")
        #expect(Group(id: home + "ow/x", sessions: []).path == home + "ow/x")
        #expect(Group(id: home + "/w", sessions: []).name == "w")
    }
}

// MARK: - 세션 파일의 세션 id (고친 버그 b)

@Suite("세션 파일의 세션 id") struct SessionIDTests {
    @Test("session_id, 없으면 파일 이름에서 .json 을 뗀 것", arguments: [
        (#"{"session_id":"abc"}"#, "abc.json", "abc"),
        (#"{}"#, "abc.json", "abc"),                                   // 예전: "abc.json"
        (#"{"state":"idle","cwd":"/p"}"#, "6f1c2a-77.json", "6f1c2a-77"),
        (#"{"session_id":"real"}"#, "other.json", "real"),             // 기록이 먼저
        (#"{"session_id":7}"#, "x.json", "x"),                         // 문자열이 아니면 없는 것으로
        (#"{}"#, "noext", "noext"),
    ])
    func 표(json: String, fileName: String, expected: String) throws {
        #expect(sessionID(try record(json), fileName: fileName) == expected)
    }
}

// MARK: - 서브에이전트 기록 해석

@Suite("서브에이전트 기록 해석") struct SubParseTests {
    @Test("시작 hook 을 본 기록만 — started_seen 이 참이 아니면 뺀다", arguments: [
        (#"{"started_seen":true,"state":"running","started":10}"#, true),
        (#"{"state":"running","started":10}"#, false),                 // 끝 hook·도구 hook 만 온 기록
        (#"{"started_seen":false,"state":"running","started":10}"#, false),
        (#"{"started_seen":"yes","state":"running","started":10}"#, false),
        (#"{"state":"done","started":10,"ended":999999}"#, false),
    ])
    func 시작_기록(json: String, shown: Bool) throws {
        let j: [String: Any] = ["a": try record(json)]
        #expect(Sub.parse(j, alive: true, now: t0).map(\.id) == (shown ? ["a"] : []))
    }

    @Test("끝난 서브에이전트는 끝난 뒤 10초까지만", arguments: [
        (0.0, true), (3.0, true), (10.0, true), (10.5, false), (11.0, false), (3600.0, false),
    ])
    func 끝난_뒤_10초(endedAgo: Double, shown: Bool) {
        let j: [String: Any] = ["a": ["started_seen": true, "state": "done", "started": 1.0,
                                      "ended": t0.timeIntervalSince1970 - endedAgo]]
        #expect(Sub.parse(j, alive: true, now: t0).map(\.id) == (shown ? ["a"] : []))
    }

    @Test("끝난 세션이거나 기록이 없으면 비어 있다") func 끝난_세션() {
        let j: [String: Any] = ["a": ["started_seen": true, "state": "running", "started": 1.0]]
        #expect(Sub.parse(j, alive: false, now: t0).isEmpty)
        #expect(Sub.parse(nil, alive: true, now: t0).isEmpty)
        #expect(Sub.parse(["a": "깨진 값"], alive: true, now: t0).isEmpty)
    }

    @Test("끝나지 않은 것 먼저, 그 안은 시작 순") func 순서() {
        let now = t0.timeIntervalSince1970
        let j: [String: Any] = [
            "done-early": ["started_seen": true, "state": "done", "started": 1.0, "ended": now - 2],
            "run-late": ["started_seen": true, "state": "running", "started": 9.0],
            "wait-mid": ["started_seen": true, "state": "waiting", "started": 5.0],
            "done-late": ["started_seen": true, "state": "done", "started": 8.0, "ended": now - 1],
            "run-early": ["started_seen": true, "state": "running", "started": 2.0],
        ]
        #expect(Sub.parse(j, alive: true, now: t0).map(\.id) == ["run-early", "wait-mid", "run-late", "done-early", "done-late"])
    }

    @Test("값이 빠지면 기본값") func 기본값() throws {
        let j: [String: Any] = ["a": ["started_seen": true, "type": "Explore", "description": "찾기", "tool": "Grep",
                                      "background": true, "started": 3.0],
                                "b": ["started_seen": true]]
        let subs = Sub.parse(j, alive: true, now: t0)
        let a = try #require(subs.first { $0.id == "a" }), b = try #require(subs.first { $0.id == "b" })
        #expect((a.type, a.desc, a.tool, a.background, a.state) == ("Explore", "찾기", "Grep", true, "running"))
        #expect(a.started == Date(timeIntervalSince1970: 3))
        #expect((b.type, b.desc, b.tool, b.background, b.state, b.ended) == ("", "", "", false, "running", nil))
        #expect(b.started == Date(timeIntervalSince1970: 0))
    }

    @Test("보드 상태 이름", arguments: [("running", "working"), ("waiting", "waiting"), ("done", "idle"), ("?", "working")])
    func 보드_상태(state: String, board: String) {
        let s = Sub(id: "a", type: "", desc: "", state: state, started: t0, ended: nil, tool: "", background: false)
        #expect(s.boardState == board)
    }
}

// MARK: - 카드에 보일 세션 (대화를 넘긴 세션 숨기기)

@Suite("카드에 보일 세션") struct VisibleSessionsTests {
    @Test("넘긴 세션은 이어 받은 세션이 있을 때만, 탭이 작업을 따라가는 동안의 탭 claude 는 늘 뺀다", arguments: [
        ([String: String](), [String](), ["a", "b"], ["a", "b"]),
        (["a": "b"], [], ["a", "b"], ["b"]),                            // 이어 받은 b 가 있으면 넘긴 a 를 뺀다
        (["a": "z"], [], ["a", "b"], ["a", "b"]),                       // 이어 받은 세션 파일이 없으면 그대로
        ([:], ["a"], ["a", "b"], ["b"]),                                // 보드 탭이 이어 받은 작업을 따라가는 동안의 탭 claude
        (["a": "b"], ["c"], ["a", "b", "c", "d"], ["b", "d"]),
        (["a": "b", "b": "c"], [], ["a", "b", "c"], ["c"]),             // 두 번 넘김
    ])
    func 표(handedTo: [String: String], parkedFrom: [String], ids: [String], expected: [String]) {
        let shown = visibleSessions(ids.map { session($0) }, handedTo: handedTo, parkedFrom: Set(parkedFrom))
        #expect(shown.map(\.id) == expected)
    }

    @Test("끝난 세션도 넘겼으면 뺀다") func 끝난_세션() {
        let shown = visibleSessions([session("a", state: "ended"), session("b", state: "working")],
                                    handedTo: ["a": "b"], parkedFrom: [])
        #expect(shown.map(\.id) == ["b"])
    }

    // handedTo 의 열쇠와 카드의 세션 id 는 같은 함수(sessionID)로 잡는다 — session_id 없는 파일이어도 서로 맞는다
    @Test("session_id 없는 세션 파일도 넘긴 세션으로 맞춘다") func 세션_id_맞추기() throws {
        let from = sessionID(try record(#"{"state":"waiting"}"#), fileName: "old-1.json")
        let to = sessionID(try record(#"{"session_id":"new-2"}"#), fileName: "new-2.json")
        let shown = visibleSessions([session(from), session(to)], handedTo: [from: to], parkedFrom: [])
        #expect(shown.map(\.id) == ["new-2"])
    }
}

// MARK: - 카드 묶음·정렬

@Suite("카드 묶음과 순서") struct CardTests {
    @Test("project_dir 별로 묶고, 카드 안은 급한 순·같으면 최근에 상태가 바뀐 것 먼저") func 묶기() throws {
        let g = groupSessions([
            session("idle", dir: "/p/a", state: "idle", since: 50),
            session("wait", dir: "/p/a", state: "waiting", since: 10),
            session("work-old", dir: "/p/a", state: "working", since: 5),
            session("ended", dir: "/p/a", state: "ended", since: 99),
            session("work-new", dir: "/p/a", state: "working", since: 30),
            session("paused", dir: "/p/a", state: "paused", since: 60),
            session("other", dir: "/p/b", state: "working"),
        ])
        #expect(g.count == 2)
        let a = try #require(g.first { $0.id == "/p/a" })
        #expect(a.sessions.map(\.id) == ["wait", "work-new", "work-old", "paused", "idle", "ended"])
        #expect(a.urgent == "waiting")
        #expect(a.active.map(\.id) == ["wait", "work-new", "work-old"])
        #expect(a.name == "a")
        #expect(try #require(g.first { $0.id == "/p/b" }).sessions.map(\.id) == ["other"])
    }

    @Test("cwd 가 달라도 project_dir 이 같으면 한 카드") func 같은_프로젝트() {
        let s1 = Session(id: "1", cwd: "/p/a/sub", projectDir: "/p/a", state: "idle", since: t0, updated: t0, headless: false,
                         tty: "", topic: "", message: "", resumeCmd: "", file: "", ctx: nil, brain: nil, pid: 0, subagents: [],
                         shells: 0, paused: false, muted: false, limitAt: nil, name: nil)
        let g = groupSessions([s1, session("2", dir: "/p/a")])
        #expect(g.map(\.id) == ["/p/a"])
        #expect(g.first?.sessions.count == 2)
    }

    // 카드 하나에 세션 하나씩: (카드 id, 상태, updated)
    @Test("카드 순서: 고정한 카드(고정한 순서) > 가장 급한 상태 > 마지막 활동이 최근", arguments: [
        ([("/idle-old", "idle", 1.0), ("/idle-new", "idle", 9.0), ("/work", "working", 0.0), ("/pin", "ended", 0.0)],
         ["/pin"], ["/pin", "/work", "/idle-new", "/idle-old"]),
        ([("/a", "waiting", 0.0), ("/b", "idle", 0.0), ("/c", "ended", 0.0)],
         ["/c", "/b"], ["/c", "/b", "/a"]),                                               // 고정한 순서 그대로
        ([("/a", "waiting", 0.0), ("/b", "working", 50.0)], [], ["/a", "/b"]),           // 급한 것이 최근 것보다 먼저
        ([("/a", "paused", 1.0), ("/b", "idle", 2.0)], [], ["/b", "/a"]),                // paused·idle 은 같은 급
        ([("/a", "idle", 1.0)], ["/없는 카드"], ["/a"]),
    ])
    func 순서(cards: [(String, String, Double)], pinned: [String], expected: [String]) {
        let groups = groupSessions(cards.map { session($0.0 + "-s", dir: $0.0, state: $0.1, updated: $0.2) })
        #expect(sortGroups(groups, pinned: pinned).map(\.id) == expected)
    }

    @Test("카드 안 세션 중 가장 급한 것·가장 최근 활동으로 견준다") func 여러_세션_카드() {
        let groups = groupSessions([
            session("a1", dir: "/a", state: "idle", updated: 100), session("a2", dir: "/a", state: "working", updated: 1),
            session("b1", dir: "/b", state: "working", updated: 50),
            session("c1", dir: "/c", state: "ended", updated: 999),
        ])
        #expect(sortGroups(groups, pinned: []).map(\.id) == ["/a", "/b", "/c"])
    }

    @Test("숨긴 세션을 빼고 묶고 정렬한다(Store.reload 끝과 같은 차례)") func 이어서() {
        let all = [
            session("old", dir: "/x", state: "waiting", updated: 5),     // /y 의 new 로 넘긴 세션
            session("new", dir: "/y", state: "working", updated: 6),
            session("z", dir: "/z", state: "idle", updated: 7),
        ]
        let groups = sortGroups(groupSessions(visibleSessions(all, handedTo: ["old": "new"], parkedFrom: [])), pinned: [])
        #expect(groups.map(\.id) == ["/y", "/z"])
    }
}

// MARK: - 세션·모델 글자

@Suite("세션·모델 글자") struct LabelTests {
    @Test("급한 정도", arguments: [("waiting", 0), ("working", 1), ("paused", 2), ("idle", 2), ("ended", 3), ("?", 9)])
    func 급한_정도(state: String, rank: Int) {
        #expect(session("a", state: state).rank == rank)
    }

    @Test("제목과 지금 하는 일", arguments: [
        ("idle", "주제", "마지막 말", "주제", "주제"),
        ("idle", "", "마지막 말", "마지막 말", "마지막 말"),
        ("idle", "", "", "요청 전", "요청 전"),
        ("waiting", "주제", "Bash 승인", "주제", "Bash 승인"),           // 승인 대기면 요청 내용
        ("waiting", "주제", "", "주제", "주제"),
    ])
    func 제목(state: String, topic: String, message: String, title: String, doingText: String) {
        let s = session("a", state: state, message: message, topic: topic)
        #expect(s.title == title)
        #expect(doing(s) == doingText)
    }

    @Test("모델 라벨", arguments: [
        ("claude-opus-5-5", "Opus", "5.5"),
        ("claude-fable-5-1", "Fable", "5.1"),
        ("claude-sonnet-4-6[1m]", "Sonnet", "4.6"),
        ("claude-haiku-4-5-20251001", "Haiku", "4.5"),
        ("gpt-x", "gpt-x", ""),
    ])
    func 모델(model: String, family: String, version: String) {
        let b = Brain(model: model, effort: "high")
        #expect(b.family == family)
        #expect(b.version == version)
    }

    @Test("effort 단계", arguments: [("low", 1), ("medium", 2), ("high", 3), ("xhigh", 4), ("max", 5), ("", 0), ("?", 0)])
    func effort(effort: String, level: Int) {
        #expect(Brain(model: "claude-opus-5-5", effort: effort).effortLevel == level)
    }

    @Test("컨텍스트 비율", arguments: [(0, 200_000, 0), (50_000, 200_000, 25), (199_999, 200_000, 99), (300_000, 200_000, 100), (5, 0, 0)])
    func 컨텍스트(used: Int, size: Int, pct: Int) {
        #expect(Ctx(used: used, size: size).pct == pct)
    }

    @Test("경과 시간", arguments: [(-5.0, "0초"), (0.0, "0초"), (59.9, "59초"), (60.0, "1분 0초"), (3599.0, "59분 59초"), (3600.0, "1시간 0분"), (7_380.0, "2시간 3분")])
    func 경과(seconds: Double, text: String) {
        #expect(elapsed(t0, t0 + seconds) == text)
    }
}
