// 사용 한도 자동 전환의 판단 — 한도에 걸린 세션 고르기, 전환할지와 그 까닭, 다음 계정과 한도 표시(full), 이어 갈 세션
// (AutoSwitcher.tick·switchAway 에 있던 판단을 옮김, 동작은 그대로, 이슈 #9 3차에 고친 것: 지금 계정의 한도 표시 기간도 떠날 까닭처럼
// 5시간·주간(전체)만 본다, 전환 실패를 까닭의 쪽으로 갈라 지금 계정·시스템 쪽이면 후보를 쓰지 않고 쉰다. 그 뒤에 고친 것: 지금 계정을 모르는데
// 떠날 까닭이 있으면 조용히 그대로 두지 않고 unknownCurrent 로 알린다). 바깥을 부르지 않는 순수 함수이고 시각은 받는다.
//
// 지금 계정의 5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 한도로 멈췄던 세션에 이어서 진행하라는 요청을 넣는다.
// 다른 계정의 남은 한도는 미리 알 수 없어(저장한 토큰은 그 계정으로 전환해야 갱신된다) 그 계정이 지금 계정이었을 때 한도가 찼던 기록(full)으로 고른다.
// 판단은 바꿀 값과 할 일만 돌려준다 — 타이머·UserDefaults(autoSwitchLast·autoSwitchFull)·로그·터미널 조작·안내·전환 실행은 앱(AutoSwitcher)이 한다
import Foundation

public enum AutoSwitchRule {
    /// 실행 중인 세션은 로그인 정보를 최대 30초 동안 기억해 두고 쓴다 — 전환한 뒤 이만큼은 이전 계정으로 요청이 나갈 수 있다
    public static let settle: TimeInterval = 40
    /// 이보다 오래된 한도 기록은 보지 않는다 — 오래전에 멈춘 세션을 계정을 바꿀 때마다 깨우지 않게
    public static let recent: TimeInterval = 6 * 3600
    /// 찬 한도가 풀리는 시각을 모를 때, 전환에 실패한 계정을 다시 고르기까지. 지금 계정·시스템 쪽 실패로 자동 전환을 쉬는 기간도 이것
    public static let retryAfter: TimeInterval = 3600
    /// 쓰기 잠금을 잡지 못해 전환에 실패했을 때 자동 전환을 쉬는 기간 — Claude Code 가 로그인 정보를 쓰는 잠깐이라 짧게
    public static let busyRetry: TimeInterval = 30
}

/// 한도에 걸린 세션 — limitAt 이 있고, 끝남·일시중지·승인 대기가 아니고(승인 대기는 Esc 가 승인을 거절하므로 건드리지 않는다), 6시간 안. 차례는 그대로
public func limitedSessions(_ sessions: [Session], now: Date) -> [Session] {
    sessions.filter {
        guard let at = $0.limitAt else { return false }
        return !["ended", "paused", "waiting"].contains($0.state) && now.timeIntervalSince(at) < AutoSwitchRule.recent
    }
}

/// 이어 가기 표지 — "세션 id@한도 시각(1970 기준 초)". 같은 한도에는 한 번만 넣는다
public func resumeKey(id: String, limitAt: Date) -> String { "\(id)@\(limitAt.timeIntervalSince1970)" }

/// 계정에서 떠나게 하는 찬 한도 — 5시간·주간(전체)이 100% 이상. 모델별 주간은 그 모델을 쓰는 세션만 막아 보지 않는다
/// (그 모델에 걸린 세션은 세션 쪽 까닭으로 떠난다). 떠날 까닭과 한도 표시 기간이 같은 판정을 쓴다
private func blocksAccount(_ l: Limit) -> Bool { (l.id == "5h" || l.id == "7d") && l.pct >= 100 }

/// 지금 계정에서 떠날 까닭 — settle 뒤에 새로 한도에 걸린 세션(첫 것)이 있거나, 5시간·주간(전체) 한도가 100% 이상이고 풀리는 시각이 미래(모르면 미래로 본다).
/// 둘 다면 세션 쪽. 떠날 까닭이 없으면 nil
public func autoSwitchReason(limited: [Session], limits: [Limit], settled: Date, now: Date) -> String? {
    if let hit = limited.first(where: { $0.limitAt.map { $0 > settled } ?? false }) {
        return "\((hit.cwd as NSString).lastPathComponent) 세션이 한도에 걸림(\(hit.message))"
    }
    if let full = limits.first(where: { blocksAccount($0) && ($0.resets ?? .distantFuture) > now }) {
        return "\(full.label) 한도 \(Int(full.pct))%"
    }
    return nil
}

/// 다음 계정과 새 한도 표시. 표시는 지난 것을 치우고, 지금 계정은 찬 5시간·주간(전체) 한도(100% 이상)의 가장 늦은 풀림 시각까지
/// (미래 것이 없거나 모르면 1시간 뒤) 표시한다 — 떠날 까닭(autoSwitchReason)과 같은 한도만 본다. 모델별 주간까지 보면 5시간이 차서 떠난
/// 계정을 며칠 뒤에 풀리는 모델별 주간 때문에 그만큼 고르지 않았다. 세션이 모델별 한도에 걸려 떠났으면 1시간 뒤 다시 고를 수 있다
/// (그 모델을 쓰는 세션이 다시 걸리면 다시 떠난다). 다음 계정은 저장 목록에서 지금 계정 다음부터 한 바퀴 돌며(지금 계정이 목록에 없으면 처음부터)
/// 지금 계정·다시 로그인해야 하는 계정·표시가 남은 계정을 뺀 첫 것. 없으면 nil — 그때도 표시는 새로 바꾼다
public func nextAccount(from current: Account, saved: [Account], limits: [Limit], full: [String: Double],
                        now: Date) -> (next: Account?, full: [String: Double]) {
    let resets = limits.filter(blocksAccount).compactMap(\.resets).filter { $0 > now }.max()
    var marks = full.filter { $0.value > now.timeIntervalSince1970 }
    marks[current.id] = (resets ?? now.addingTimeInterval(AutoSwitchRule.retryAfter)).timeIntervalSince1970
    let start = saved.firstIndex { $0.id == current.id }.map { $0 + 1 } ?? 0
    let next = (saved[start...] + saved[..<start]).first { $0.id != current.id && !$0.isExpired(at: now) && marks[$0.id] == nil }
    return (next, marks)
}

/// 이어 갈 세션 하나 — 앱이 key 를 이어 가기 표지에 넣고 터미널에 요청을 넣는다
public struct AutoResume: Equatable, Sendable {
    public let key: String
    public let session: Session
    public init(key: String, session: Session) { self.key = key; self.session = session }
}

/// 이어 가기 — 전환하고 settle 이 지났으면, 그 전(이전 계정)에 한도로 멈춘 세션 가운데 아직 이어 가지 않은 것. 차례는 그대로.
/// 전환 대기·전환 중·지금 계정과 관계없다
public func resumeTargets(limited: [Session], settled: Date, resumed: Set<String>, now: Date) -> [AutoResume] {
    guard now > settled else { return [] }
    return limited.compactMap { s in
        guard let at = s.limitAt, at <= settled else { return nil }
        let key = resumeKey(id: s.id, limitAt: at)
        return resumed.contains(key) ? nil : AutoResume(key: key, session: s)
    }
}

/// 자동 전환으로 할 일
/// Account 가 Sendable 이 아니라(oauth 가 [String: Any]) 이것도 Sendable 이 아니다 — 앱의 메인 스레드 타이머에서만 쓴다
public enum AutoSwitchMove: Equatable {
    case stay
    /// next 로 바꾼다. stopped: 한도로 멈춘 세션이 있나(끝나지 않았으면 승인 대기·일시중지·6시간 넘은 것도) — 안내 글자에 "이어서 진행" 을 붙인다
    case switchTo(next: Account, from: Account, reason: String, stopped: Bool)
    /// 떠나야 하는데 바꿀 계정이 없다
    case noCandidate(reason: String)
    /// 떠나야 하는데 지금 로그인된 계정을 모른다(계정 정보가 없거나 계정 id 가 빠짐) — 다음 계정을 고를 수 없어 바꾸지 않는다. 한도 표시는 그대로
    case unknownCurrent(reason: String)
}

public struct AutoSwitchDecision: Equatable {
    public let move: AutoSwitchMove
    public let full: [String: Double]?   // 새 한도 표시 — 떠나려 했으면(후보가 없어도) 새 값, 아니면 nil(그대로)
    public let resume: [AutoResume]      // 이어 갈 세션 — 전환과 함께 나올 수 있다(앱은 전환을 먼저 요청하고 이어 간다)
    public init(move: AutoSwitchMove, full: [String: Double]?, resume: [AutoResume]) {
        self.move = move; self.full = full; self.resume = resume
    }
}

/// 자동 전환을 쉬는 동안 — 지금 계정이 쉬기 시작한 계정(account)이고 until 전이면 전환을 판단하지 않는다(이어 가기는 그대로).
/// 지금 계정이 바뀌거나 until 이 되면 끝난다
public struct AutoSwitchPause: Equatable, Sendable {
    public let account: String
    public let until: Date
    public init(account: String, until: Date) { self.account = account; self.until = until }
    /// 지금 쉬는 중인가
    public func holds(current: String?, now: Date) -> Bool { current == account && now < until }
}

/// 자동 전환 요청이 실패한 뒤의 한도 표시와 쉼. 대상 계정 쪽이면 그 계정만 retryAfter 동안 고르지 않게 표시하고 쉬지 않는다
/// (다음 판단이 다음 후보를 고른다). 그 밖(지금 계정·시스템 쪽)이면 다른 계정으로 해도 같은 까닭으로 실패하므로 표시는 그대로 두고
/// 지금 계정이 바뀌거나 retryAfter 가 지날 때까지 쉰다 — 대상 쪽처럼 표시하면 3초마다 다음 후보를 하나씩 표시해 모두 쓰고, 그 사이
/// 실패 안내가 거듭 뜬다. 쓰기 잠금을 잡지 못했으면(busy) 곧 풀리므로 표시는 그대로 두고 busyRetry 만 쉰 뒤 같은 후보부터 다시 판단한다
public func autoSwitchFailed(_ side: SwitchFailureSide, target: Account, from current: Account, full: [String: Double],
                             now: Date) -> (full: [String: Double], pause: AutoSwitchPause?) {
    let until = now.addingTimeInterval(AutoSwitchRule.retryAfter)
    switch side {
    case .target:
        var marks = full
        marks[target.id] = until.timeIntervalSince1970
        return (marks, nil)
    case .other:
        return (full, AutoSwitchPause(account: current.id, until: until))
    case .busy:
        return (full, AutoSwitchPause(account: current.id, until: now.addingTimeInterval(AutoSwitchRule.busyRetry)))
    }
}

/// 3초마다 한 번의 판단. 전환은 settle 이 지났고, 전환 대기(pending)·전환 중(switching)·쉬는 중(pause)이 아닐 때만 본다.
/// 지금 계정을 모르면(nil — 계정 정보가 없거나 계정 id 가 빠짐) 다음 계정을 고를 수 없어 바꾸지 않고, 떠날 까닭이 있으면 unknownCurrent 로
/// 알린다(표시는 그대로 — 전에는 조용히 그대로 두어 한도가 차도 아무 일이 없었다). 그때는 쉬기 시작한 계정과 견줄 수 없어 쉬는 중을 끝 시각으로만 본다.
/// sessions 는 보드 카드의 세션 모두, limits 는 지금 계정의 사용량, lastSwitch 는 마지막으로 계정을 바꾼 시각(손으로 바꾼 것 포함),
/// pause 는 지난 전환 실패(autoSwitchFailed)로 쉬는 것 — 쉬는 동안은 표시도 바꾸지 않고, 이어 가기는 그대로 본다
public func decideAutoSwitch(sessions: [Session], limits: [Limit], current: Account?, saved: [Account],
                             full: [String: Double], resumed: Set<String>, lastSwitch: Date,
                             pending: Bool, switching: Bool, pause: AutoSwitchPause?, now: Date) -> AutoSwitchDecision {
    let settled = lastSwitch.addingTimeInterval(AutoSwitchRule.settle)
    let limited = limitedSessions(sessions, now: now)
    var move = AutoSwitchMove.stay
    var marks: [String: Double]?
    let resting = current.map { pause?.holds(current: $0.id, now: now) ?? false } ?? (pause.map { now < $0.until } ?? false)
    // 1) 전환 — 지금 계정으로 새로 한도에 걸린 세션이 있거나, 지금 계정의 5시간·주간 한도가 찼다
    if !pending, !switching, !resting, now > settled,
       let reason = autoSwitchReason(limited: limited, limits: limits, settled: settled, now: now) {
        if let cur = current {
            let picked = nextAccount(from: cur, saved: saved, limits: limits, full: full, now: now)
            marks = picked.full
            if let next = picked.next {
                let stopped = sessions.contains { $0.limitAt != nil && $0.state != "ended" }
                move = .switchTo(next: next, from: cur, reason: reason, stopped: stopped)
            } else {
                move = .noCandidate(reason: reason)
            }
        } else {
            move = .unknownCurrent(reason: reason)
        }
    }
    // 2) 이어 가기
    let resume = resumeTargets(limited: limited, settled: settled, resumed: resumed, now: now)
    return AutoSwitchDecision(move: move, full: marks, resume: resume)
}
