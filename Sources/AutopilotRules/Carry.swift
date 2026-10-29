// 자율주행 규칙(AutopilotRules) — 지난 실행 이어 가기의 미룬 사람 몫(next_start). 2026-10-30 2단계 5-2b 에서 새로 씀.
// 이어 가기는 고른 실행과 그 실행이 이어 간 줄(continues 를 거슬러)의 미룬 몫을 출발 카드로 가져온다(5-2a). 그 줄 밖의 실행(이어 가지 않은 옛 실행,
// 같은 프로젝트의 다른 갈래)에서 미룬 몫도 아직 열려 있으면 보인다(apOpenParked). 기록 저장소는 보드가 읽고 값으로 넘긴다
import Foundation

// 미룬 몫 한 줄(기록 저장소 carry) — 어느 실행의 몇째 단위가 남겼나
public struct APCarryRow: Equatable, Sendable {
    public var id: String
    public var run: String
    public var unit: Int
    public var kind: String
    public var title: String
    public var why: String
    public var started: Double?     // 그 실행의 출발 시각(최근 것부터 보이려고)
    public init(id: String, run: String, unit: Int, kind: String, title: String, why: String, started: Double?) {
        self.id = id; self.run = run; self.unit = unit; self.kind = kind; self.title = title; self.why = why; self.started = started
    }
}

// 이어 간 줄 밖의 열린 미룬 몫 — 같은 프로젝트의 next_start 가운데 줄(chain: 고른 실행과 그것이 이어 간 실행들) 밖의 실행이 남긴 것,
// 어느 실행의 출발 카드에서도 정함(decide)·닫음(close)으로 답하지 않은 것(다시 미룸 park 은 열린 채다), 이미 카드로 가져온 것(shown 의 id·제목)과
// 제목이 같은 것은 뺀다. 같은 제목이 여럿이면 최근 실행의 것 하나. 최근 실행부터
public func apOpenParked(rows: [APCarryRow], answers: [String: String], chain: Set<String>, shown: [(id: String, title: String)]) -> [APCarryRow] {
    let shownIds = Set(shown.map(\.id)), shownTitles = Set(shown.map(\.title))
    var out: [APCarryRow] = [], titles = Set<String>()
    for r in rows.sorted(by: { ($0.started ?? 0, $0.run, -$0.unit) > ($1.started ?? 0, $1.run, -$1.unit) })
        where r.kind == "next_start" && !chain.contains(r.run) && !["decide", "close"].contains(answers[r.id] ?? "")
            && !shownIds.contains(r.id) && !shownTitles.contains(r.title) {
        guard titles.insert(r.title).inserted else { continue }
        out.append(r)
    }
    return out
}
