// 세션 조작의 순서와 판단 — 일시중지·재개·중단·보드로 옮기기·이어 열기·한 번 누르기의 갈래, 끝나기 기다리기
// (ClaudeBoard.swift 의 pauseSession·resumePaused·stopSession·moveToBoard·resumeSession·openSession·waitExit 에 있던 것을 옮김,
// 동작·포트 호출 차례·안내 글자는 그대로, 옮긴 뒤 고친 것: 중단은 끝난 세션·이미 없는 pid 에 신호를 보내지 않는다,
// 재개는 살아 있는 세션에 키를 넣지 못해도 이어 열지 않고 알린다).
// 확인 창·애니메이션 기다리기(whenPaneSettled)·큐·타이머는 앱이 쥐고, 확인을 받은 뒤 여기를 부른다.
// 바깥은 포트로만 부른다: 세션 터미널(단계 넣기·끝나기 기다리기·포커스), 보드 탭, 프로세스, 세션 표지, 시계.
//
// 할 일 목록 값 대신 포트를 받는 유스케이스로 둔 까닭: 흐름이 "단계를 넣고 → 넣었는지에 따라 표지·신호·끝나기 기다리기 → 그 결과에 따라 이어 열기·안내"
// 처럼 앞 결과에 달린 사슬이다. 값으로 돌려주면 앱에 사슬을 다시 엮는 해석기가 생기고 그 해석기는 시험 밖에 남는다.
// 가짜 포트가 done 을 그 자리에서 부르면 사슬이 바로 풀려, 부른 차례가 한 줄 목록으로 남고 그대로 견줄 수 있다
import Foundation

/// 세션 조작이 넣는 단계
public enum SessionSteps {
    /// 일시중지 — Esc 로 지금 응답만 멈춘다. 그때까지 한 작업은 대화 기록에 남아 재개하면 바로 이어 간다
    public static let pause: TerminalSteps = [("key", "escape")]
    /// 끝내기(중단·보드로 옮기기) — Esc 로 응답을 멈추고 /exit
    public static let exit: TerminalSteps = [("key", "escape"), ("wait", "0.4"), ("text", "/exit"), ("wait", "0.2"), ("key", "enter")]
    /// 살아 있는 세션에 요청 글을 넣고 보낸다
    public static func prompt(_ text: String) -> TerminalSteps { [("text", text), ("wait", "0.2"), ("key", "enter")] }
}

/// 일시중지했던 세션을 재개할 때 넣는 요청 — 살아 있으면 그 터미널에, 꺼졌으면 이어 여는 claude 의 첫 요청으로
public let carryOnPrompt = "보드에서 일시중지했던 세션을 재개합니다. 중단된 지점부터 직전 작업을 이어서 진행해 주세요."

/// 일시중지 표지(sessions/<id>.paused) 내용 — {"paused_at": 1970 기준 초, "topic": 주제}. 키 차례는 정하지 않는다(읽는 쪽은 JSON 으로 푼다)
public func pausedMarkData(topic: String, at: Date) -> Data? {
    let mark = ["paused_at": at.timeIntervalSince1970, "topic": topic] as [String: Any]
    return try? JSONSerialization.data(withJSONObject: mark)
}

/// 보드 탭에서 이어 열 claude 명령의 바탕 — state.py 가 기록한 resume_cmd(실행 인자·마지막 권한 모드·모델, -p 는 빠짐), 없으면 claude --resume <id>
public func resumeBaseCommand(_ s: Session) -> String {
    s.resumeCmd.isEmpty ? "claude --resume \(s.id)" : s.resumeCmd
}

/// 이어서 진행하라는 요청을 첫 요청으로 붙인다 — 작은따옴표로 감싸고 안의 ' 는 '\''
public func carryOnCommand(_ command: String, prompt: String = carryOnPrompt) -> String {
    command + " '" + prompt.replacingOccurrences(of: "'", with: "'\\''") + "'"
}

/// 보드로 옮기기 전에 물어야 하는가 — 작업 중·승인 대기면 진행 중인 응답이 끊긴다
public func moveNeedsConfirm(_ s: Session) -> Bool { s.state == "working" || s.state == "waiting" }

/// 백그라운드 세션을 눌렀을 때 안내 — 실행 방식을 모르면 -p
public func backgroundNote(_ s: Session, mode: String?) -> String {
    "\(s.title): claude \(mode ?? "-p") 로 실행된 세션이라 터미널이 없습니다. 보드에서는 상태만 보여 주고, 업데이트·종료에도 영향받지 않습니다."
}

/// 끝나기를 기다리는 한 번의 판단 — 앱이 간격마다 다시 부른다(타이머는 앱)
public enum ExitWait: Equatable, Sendable {
    case exited                                   // 끝났다(pid 0·없는 pid 포함)
    case timedOut                                 // 남은 시간이 없다
    case poll(after: Double, remaining: Double)   // after 초 뒤 remaining 으로 다시 본다
}

/// 끝나기 기다리기 — pid 가 0 이하이거나 살아 있지 않으면(EPERM 도) 끝남, 남은 시간이 0 이하면 시간 초과, 아니면 0.2초 뒤 남은 시간에서 0.2 를 빼고 다시.
/// 8초면 40번 다시 본 뒤 시간 초과다(0.2 를 빼 가는 부동소수 그대로)
public func exitWait(pid: Int32, timeout: Double, processes: ProcessControl) -> ExitWait {
    guard pid > 0, processes.isAlive(pid) else { return .exited }
    guard timeout > 0 else { return .timedOut }
    return .poll(after: 0.2, remaining: timeout - 0.2)
}

/// 세션 조작 유스케이스 — 앱의 @MainActor 함수가 메인에서 부른다(포트의 done 도 메인에서 온다)
public struct SessionControl: Sendable {
    private let terminals: SessionTerminals
    private let tabs: BoardTabs
    private let processes: ProcessControl
    private let marks: SessionMarks
    private let clock: Clock

    public init(terminals: SessionTerminals, tabs: BoardTabs, processes: ProcessControl, marks: SessionMarks, clock: Clock) {
        self.terminals = terminals; self.tabs = tabs; self.processes = processes; self.marks = marks; self.clock = clock
    }

    /// 일시중지 — Esc 를 넣고, 넣었으면 표지를 남긴다(시각은 넣은 뒤). 못 넣으면 아무것도 하지 않는다
    public func pause(_ s: Session) {
        terminals.drive(s, SessionSteps.pause) { ok in
            guard ok else { return }
            if let data = pausedMarkData(topic: s.topic, at: clock.now) { marks.writePaused(s.id, data) }
        }
    }

    /// 재개 — 살아 있는 세션은 그 터미널에 이어서 진행하라는 요청을 넣고(넣으면 표지를 지운다), 꺼진 세션은 보드 새 탭에서 이어 연다.
    /// 살아 있음 = 끝나지 않았고 (pid 가 없거나 pid 가 살아 있음). pid 없는 세션은 판정(scanSessions)이 Claude Code 세션 파일로 생사를 보고
    /// 끝나지 않았다고 둔 것이라 살아 있다고 본다. pid 확인을 남기는 까닭: 보드가 다시 뜬 직후 판정이 아직 끝남으로 바꾸지 못한 1초 사이에는
    /// 죽은 pid 의 세션을 이어 열어야 한다.
    /// 살아 있는데 넣지 못하면 이어 열지 않고(같은 대화의 claude 가 둘이 된다) 그 터미널에서 이어 가라고 알리기만 하고 표지는 그대로 둔다
    public func resumePaused(_ s: Session) {
        let alive = s.state != "ended" && (s.pid <= 0 || processes.isAlive(s.pid))
        guard alive else { return reopen(s, carryOn: true) }
        let name = (s.cwd as NSString).lastPathComponent
        terminals.drive(s, SessionSteps.prompt(carryOnPrompt)) { ok in
            if ok { marks.clearPaused(s.id) } else { tabs.toast("\(name): 세션이 아직 돌고 있지만 그 터미널에 키를 넣지 못했습니다. 그 터미널에서 이어 가 주세요") }
        }
    }

    /// 중단(확인을 받은 뒤) — 일시중지 표지를 지우고, 진짜 백그라운드 세션과 끝난 세션은 더 건드리지 않는다. 그 밖은 끝내기 단계를 넣고,
    /// 못 넣으면(보드 탭 안의 claude -p·Ghostty 밖 터미널 포함) 기록의 pid 가 아직 살아 있을 때만 SIGTERM.
    /// 끝난 세션에 신호를 보내지 않는 까닭: 끝난 카드는 이틀 남는데 그 사이 기록의 pid 를 다른 프로세스가 다시 쓸 수 있다
    /// (메뉴는 끝난 세션에 중단을 보이지 않지만 --action stop <id> 는 확인 없이 여기로 온다).
    /// 가르는 것은 place 다 — 기록은 끝났어도 보드 탭이 그 세션의 claude 를 돌리고 있으면 탭이라 끝내기 단계를 넣는다
    public func stop(_ s: Session) {
        marks.clearPaused(s.id)
        let place = tabs.place(s)
        if place == .background || place == .ended { return }
        terminals.drive(s, SessionSteps.exit) { ok in
            if !ok, s.pid > 0, processes.isAlive(s.pid) { processes.terminate(s.pid) }
        }
    }

    /// 다른 터미널(Ghostty)에서 도는 세션을 보드로(확인을 받은 뒤) — 끝내기 단계를 넣고 끝나기를 8초까지 기다려 보드 새 탭에서 이어 연다(대화 그대로)
    public func moveToBoard(_ s: Session) {
        let name = (s.cwd as NSString).lastPathComponent
        terminals.drive(s, SessionSteps.exit) { ok in
            guard ok else { return tabs.toast("\(name): 세션 터미널을 찾지 못해 보드로 옮기지 못했습니다") }
            terminals.waitForExit(s.pid, timeout: 8) { exited in
                if exited { reopen(s) } else { tabs.toast("\(name): 세션이 끝나지 않아 보드로 옮기지 못했습니다. 그 터미널을 확인해 주세요") }
            }
        }
    }

    /// 끝난 세션을 보드 탭에서 그 세션의 프로젝트 경로(claude 를 띄운 폴더)로 이어 연다 — 그 세션이 마지막으로 돌던 화면이 같은 경로의
    /// 셸 프롬프트로 남아 있으면 그 화면에, 아니면 새 탭에. carryOn 이면 이어서 진행하라는 요청을 붙이고 일시중지 표지를 지운다.
    /// 차례: 명령(답변 꾸미기) → 요청 붙이기·표지 지우기 → 화면 찾기 → 넣기·보이기 또는 새 탭
    public func reopen(_ s: Session, carryOn: Bool = false) {
        let decorated = tabs.claudeCommand(resumeBaseCommand(s), session: s.id)
        var command = decorated.command
        let off = decorated.mdRenderOff
        if carryOn {
            command = carryOnCommand(command)
            marks.clearPaused(s.id)
        }
        if let screen = tabs.promptScreen(session: s.id, dir: s.projectDir) {
            tabs.run(command, in: screen, session: s.id, mdRenderOff: off)
            tabs.reveal(screen)
        } else {
            tabs.launch(command, in: s.projectDir, session: s.id, mdRenderOff: off)
        }
    }

    /// 세션을 한 번 눌렀을 때(목록·미니·알림 패널 "세션으로 이동") — 넘긴 대화를 붙여 보여 주는 화면이 먼저, 보드 탭이면 그 화면으로,
    /// 끝난 세션은 보드 새 탭에서 이어 열기(끝난 백그라운드 세션은 아무것도 하지 않는다), 백그라운드 세션은 안내만,
    /// 다른 터미널 세션은 그 앱으로(두 번 누르면 그 세션의 터미널 창까지)
    public func open(_ s: Session, double: Bool) {
        if let id = tabs.parkedTab(s) { return tabs.reveal(id) }
        switch tabs.place(s) {
        case .tab(let id, _):
            tabs.reveal(id)
        case .ended:
            if !s.headless { reopen(s, carryOn: s.paused) }
        case .background:
            tabs.toast(backgroundNote(s, mode: tabs.mode(s)))
        case .terminal:
            if double { terminals.focus(s) } else { terminals.bringForward(s) }
        }
    }
}
