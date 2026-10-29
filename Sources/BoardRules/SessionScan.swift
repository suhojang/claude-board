// 세션 상태 판정 유스케이스 — 세션 기록을 읽어 보드에 보일 세션 상태를 정한다(Store.reload 앞부분이던 것을 옮김, 동작은 그대로).
// 바깥은 포트(Ports.swift)로만 부른다. 묶음·정렬·숨김·화면 갱신은 Store.reload 에 남는다
import Foundation

public struct SessionScan: Sendable {
    public let sessions: [Session]          // 세션 기록 차례 그대로 — 낡은 기록은 지우고 뺐다
    public let handedTo: [String: String]   // 대화를 백그라운드로 넘긴 세션 id → 이어 받은 세션 id
    public let now: Date                    // 이번 판정에 쓴 시각 — 시계는 한 번만 읽고 Store 도 이 시각을 쓴다
    public init(sessions: [Session], handedTo: [String: String], now: Date) {
        self.sessions = sessions; self.handedTo = handedTo; self.now = now
    }
}

// 서브에이전트의 대화 기록 — 세션 대화 기록에서 .jsonl 을 뗀 폴더 아래 subagents/agent-<id>.jsonl
public func subagentTranscript(_ transcript: String, agent: String) -> String {
    let subDir = ((transcript.hasSuffix(".jsonl") ? String(transcript.dropLast(6)) : transcript) as NSString)
        .appendingPathComponent("subagents")
    return (subDir as NSString).appendingPathComponent("agent-\(agent).jsonl")
}

// 카드 제목·마지막 말에서 마크다운 표시(# * `)를 뺀다
private func clean(_ s: String) -> String {
    s.replacingOccurrences(of: "[#*`]+", with: "", options: .regularExpression)
        .trimmingCharacters(in: .whitespaces)
}

// 세션 기록마다 상태를 정한다. 차례는 아래 주석대로이고, 포트를 부르는 조건(언제 대화 기록을 stat·읽는지)도 예전 그대로다 —
// 1초마다 도는 길이라 부르는 횟수가 곧 보드 CPU 다
public func scanSessions(records: SessionRecords, processes: ProcessProbe, live: ClaudeLive,
                         transcripts: Transcripts, contexts: ContextUsage, clock: Clock) -> SessionScan {
    let now = clock.now
    var sessions: [Session] = []
    var brainPaths = Set<String>()        // 모델·중단 캐시에 남길 대화 기록
    var transcriptPaths = Set<String>()   // 넘김 캐시에 남길 대화 기록
    var handedTo: [String: String] = [:]  // 대화를 백그라운드로 넘긴 세션 → 이어 받은 세션 id
    var livePids = Set<Int32>()           // Claude Code 상태 캐시에 남길 pid — 목록에 남은 세션의 판정 pid
    for record in records.load() {
        let j = record.json
        var state = j["state"] as? String ?? "idle"
        let updated = Date(timeIntervalSince1970: j["updated"] as? Double ?? 0)
        let sid = sessionID(j, fileName: record.name)
        // 판정에 쓸 pid — 기록의 pid. state.py 가 claude 를 못 찾아 기록에 pid 가 없으면 Claude Code 의 세션 파일
        // (~/.claude/sessions/<pid>.json) 가운데 sessionId 가 이 세션인 것의 pid. 끝난 세션은 찾지 않는다(1초마다 도는 길이다).
        // 찾은 pid 는 이 판정(생사·Claude Code status·이름·셸 수)에만 쓰고 Session.pid 는 기록 그대로 둔다 — Session.pid 는
        // 중단의 SIGTERM·끝나기 기다리기·탭 짝짓기·재개의 대상이라, 그 동작을 바꾸는 일은 이 판정 밖이다
        let recordPid = (j["pid"] as? NSNumber)?.int32Value
        let livePid = recordPid ?? (state != "ended" ? live.pid(forSession: sid) : nil)
        // pid 가 확실히 없을 때만 끝난 것으로 본다(권한이 없어 못 보는 pid 는 그대로). 기록에 pid 가 없고 Claude Code 세션 파일에서도
        // 못 찾으면 끝났다고 단정하지 않는다(claude -p 처럼 그 파일을 쓰지 않는 세션이 있을 수 있다)
        if state != "ended", let livePid, processes.isGone(livePid) {
            state = "ended"
        }
        // 질문 없이 끝난 세션은 대화 기록 파일이 없어 이어 열 수 없으므로 목록에서 뺀다.
        // 지우는 것은 끝난 세션 중 (이틀 넘었거나 대화 기록이 없는) 것뿐이다 — 쉼표가 && 라 끝나지 않은 세션은 대화 기록이 없어도 남는다
        // (첫 요청 전에는 대화 기록이 아직 없다). 대화 기록은 끝난 지 이틀 안 된 세션만 확인한다
        let cwdPath = j["cwd"] as? String ?? "?"
        let transcript = transcripts.locate(given: j["transcript_path"] as? String, cwd: cwdPath, session: sid)
        if state == "ended", now.timeIntervalSince(updated) > 2 * 86400
            || !transcripts.exists(transcript) {
            records.remove(record)
            continue
        }
        // Esc 로 끊긴 세션은 입력을 기다리는 중이다 — 그 뒤에도 도는 서브에이전트·셸이 있으면 아래에서 다시 작업 중이 된다.
        // 첫 응답이 오기 전에 끊으면 대화 기록에 중단 표시가 남지 않으므로 Claude Code 의 status 가 이번 상태 뒤에 idle 이 된 것도 본다
        let pid = livePid ?? 0
        livePids.insert(pid)
        var since = Date(timeIntervalSince1970: j["since"] as? Double ?? 0)
        let status = live.statusChange(pid, session: sid).flatMap { $0.at > since ? $0 : nil }
        if state == "working" || state == "waiting",
           let at = transcripts.interruptedAt(transcript) ?? (status?.status == "idle" ? status?.at : nil) {
            state = "idle"
            since = at
        } else if state == "waiting", let status, status.status == "busy" {
            // 승인한 도구가 도는 동안은 hook 이 없다(PostToolUse 까지) — 승인 창이 닫혀 Claude Code 가 다시 busy 면 작업 중이다
            state = "working"
            since = status.at
        }
        // 응답은 끝났어도 서브에이전트나 백그라운드 셸이 돌고 있으면 작업 중으로 보인다
        let subs = Sub.parse(j["subagents"] as? [String: Any], alive: state != "ended", now: now).map { sub -> Sub in
            var sub = sub
            let p = subagentTranscript(transcript, agent: sub.id)
            brainPaths.insert(p)
            sub.brain = transcripts.brain(p, fallbackModel: "", now: now)
            return sub
        }
        let shellCount = state == "idle" ? processes.backgroundShells(pid) : 0   // 응답 중에 도는 셸은 보통 도구 호출이라 세지 않는다
        if state == "idle", shellCount > 0 || subs.contains(where: { $0.state != "done" }) {
            state = "working"
        }
        // 일시중지 표지는 보드가 남기고, 새 요청이 들어오면 state.py 가 지운다
        let paused = records.isPaused(sid)
        if paused, state != "ended" { state = "paused" }
        let muted = records.isMuted(sid)
        var brain: Brain? = nil
        if state != "ended" {
            brainPaths.insert(transcript)
            brain = transcripts.brain(transcript, fallbackModel: j["model"] as? String ?? "", now: now)
            if let b = brain, let e = contexts.liveEffort(sid, transcript: transcript) { brain = Brain(model: b.model, effort: e) }
        }
        // 대화를 백그라운드로 넘긴 세션(끝난 것도) — 넘긴 뒤에 이 세션이 다시 열렸으면(--resume 의 SessionStart 로 started 가 새로 잡힘)
        // 아직 대화 줄이 없어도 넘긴 세션으로 보지 않는다
        transcriptPaths.insert(transcript)
        if let to = transcripts.handoff(transcript), to.id != sid,
           (j["started"] as? Double ?? 0) <= to.at.timeIntervalSince1970 {
            handedTo[sid] = to.id
        }
        sessions.append(Session(id: sid,
                                cwd: j["cwd"] as? String ?? "?",
                                projectDir: j["project_dir"] as? String ?? j["cwd"] as? String ?? "?",
                                state: state,
                                since: since,
                                updated: updated,
                                headless: j["headless"] as? Bool ?? false,
                                tty: j["tty"] as? String ?? "",
                                topic: clean(j["topic"] as? String ?? ""),
                                message: clean(j["last_message"] as? String ?? ""),
                                resumeCmd: j["resume_cmd"] as? String ?? "",
                                file: record.file,
                                ctx: contexts.read(sid, transcript: transcript, model: j["model"] as? String ?? "", now: now),
                                brain: brain,
                                pid: recordPid ?? 0,
                                subagents: subs,
                                shells: shellCount,
                                paused: paused,
                                muted: muted,
                                limitAt: ((j["limit_hit"] as? [String: Any])?["at"] as? Double).map { Date(timeIntervalSince1970: $0) },
                                name: state == "ended" ? nil : live.name(pid)))
    }
    live.prune(keep: livePids)
    transcripts.prune(keep: brainPaths, keepHandoffs: transcriptPaths)
    // 컨텍스트는 끝난 세션 카드에도 보이므로 목록에 남은 모든 세션 몫을 남긴다(지운 기록은 뺀다)
    contexts.prune(keep: Set(sessions.map(\.id)), keepTranscripts: transcriptPaths)
    return SessionScan(sessions: sessions, handedTo: handedTo, now: now)
}
