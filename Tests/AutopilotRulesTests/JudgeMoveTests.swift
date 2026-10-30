// ⑪-1 옮기기 시험: 엔진 fixedCommand·plainCommand 의 출력 끝 식과 cmdPrerun 의 출발 카드 감을 규칙(Checks.swift 의 apOutputTail·apPrerunCard)으로 옮겼다.
// 옮기기 전 식(06b2731 엔진의 글 그대로)을 여기 두고, 만든 입력 쉰셋 꼴(빈 것·줄 수·긴 줄·4000 바이트 경계에서 잘리는 한글·줄바꿈 꼴)과
// 카드 입력(단위 번호·id·제목)으로 옮긴 뒤 결과와 같은지 본다(이슈 #9 의 덤프 대조를 시험으로 둔 것)
import Foundation
import Testing
import AutopilotRules

// 옮기기 전 엔진의 식 그대로
private func oldTail(_ d: Data) -> String {
    var tail = String(decoding: d.suffix(4000), as: UTF8.self)
    tail = tail.split(separator: "\n", omittingEmptySubsequences: false).suffix(40).joined(separator: "\n")
    return tail
}
private func oldCard(_ d: [String: Any], _ n: Int, _ t: String) -> [String: Any] {
    ["id": "prerun-\(d["id"] ?? "u\(n)")", "unit": n, "kind": "prerun",
     "title": "「\(t)」 의 판정이 일하기 전부터 통과합니다",
     "why": "이미 끝난 일이거나 판정이 약하다는 뜻입니다. 그대로 두면 모델이 일하지 않아도 통과로 기록됩니다.",
     "options": [["key": "drop", "label": "빼기"], ["key": "fix_check", "label": "판정 고치기"], ["key": "keep", "label": "그대로"]],
     "recommended": "drop"]
}

private func inputs() -> [Data] {
    var out: [Data] = [Data(), Data("\n".utf8), Data("한 줄".utf8), Data("끝 줄바꿈\n".utf8), Data("\r\n섞임\r\n".utf8)]
    for lines in [1, 39, 40, 41, 120] {
        for width in [0, 3, 80, 400] {
            let line = String(repeating: "가", count: width / 3) + String(repeating: "a", count: width % 3)
            out.append(Data((0..<lines).map { "\($0) \(line)" }.joined(separator: "\n").utf8))
            out.append(Data(((0..<lines).map { "\($0) \(line)" }.joined(separator: "\n") + "\n").utf8))
        }
    }
    for shift in 0..<8 {   // 4000 바이트 경계가 한글(3 바이트) 가운데에 오게
        out.append(Data((String(repeating: "x", count: shift) + String(repeating: "판정", count: 900)).utf8))
    }
    return out
}

private func json(_ o: Any) -> String {
    String(decoding: (try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys])) ?? Data(), as: UTF8.self)
}

@Suite("⑪-1 옮기기: 출력 끝과 미리 돌리기 카드") struct JudgeMoveTests {
    @Test("출력 끝: 옮기기 전 식과 같음(53 꼴)")
    func 출력끝() {
        let all = inputs()
        #expect(all.count == 53)
        for d in all { #expect(apOutputTail(d) == oldTail(d)) }
    }

    @Test("미리 돌리기 카드: 옮기기 전 식과 같음", arguments: [(1, "u1", "할 일 목록 API"), (3, "fix-12", "「따옴표」 와 \"글\""), (12, "u12", "")])
    func 카드(_ a: (Int, String, String)) {
        #expect(json(apPrerunCard(n: a.0, id: a.1, title: a.2)) == json(oldCard(["id": a.1], a.0, a.2)))
    }
}
