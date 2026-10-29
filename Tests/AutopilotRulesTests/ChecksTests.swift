// 판정 실패 글 규칙(Sources/AutopilotRules/Checks.swift) 시험.
// 2026-10-30 ⑦ 에서 엔진 runUnit·status 에서 옮길 때 옮기기 전 식(10d145e)과 같은 입력 253줄(종료 코드 여섯 × 다시 맡긴 횟수 넷 × 다시 맡길 수 넷,
// 종료 코드 여섯 × 출력 끝 여섯 × 판정 명령 넷, 다시 맡긴 횟수 다섯, check_timeout_min 꼴 여덟)로 돌린 덤프가 같았다
import Testing
import AutopilotRules

struct CheckEventRow: Row {
    let note: String
    let code: Int32
    let tries: Int
    let retries: Int
    let want: String
}

private let eventRows: [CheckEventRow] = [
    .init(note: "첫 판정 실패 → 다시 맡김 1/2", code: 1, tries: 0, retries: 2, want: "판정 실패(종료 코드 1). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2)."),
    .init(note: "둘째 실패 → 2/2", code: 3, tries: 1, retries: 2, want: "판정 실패(종료 코드 3). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(2/2)."),
    .init(note: "횟수가 다 찼는데 다시 맡기면 사람의 말 때문", code: 1, tries: 2, retries: 2,
          want: "판정 실패(종료 코드 1). 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다."),
    .init(note: "지금 동작: 시간 상한에 닿아도 종료 코드 -1 의 실패로만 적음", code: -1, tries: 0, retries: 2,
          want: "판정 실패(종료 코드 -1). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(1/2)."),
]

@Suite("자율주행 판정 실패 글") struct CheckTextTests {
    @Test("진행 기록 글", arguments: eventRows)
    func 진행기록(_ r: CheckEventRow) {
        #expect(apCheckFailEvent(code: r.code, tries: r.tries, retries: r.retries) == r.want)
    }

    @Test("모델에게 넘기는 글: 종료 코드·판정 명령·출력 끝을 그대로")
    func 넘기는글() {
        #expect(apCheckFailMessage(code: 2, check: "test -f a.txt", tail: "없음") == """
        판정 명령이 실패했습니다(종료 코드 2). 판정 명령: `test -f a.txt`
        아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.

        ```
        없음
        ```
        """)
    }

    @Test("미통과 까닭: 다시 맡긴 횟수")
    func 미통과() {
        #expect(apCheckFailReason(tries: 0) == "판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.")
        #expect(apCheckFailReason(tries: 2) == "2번 다시 해도 판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다.")
    }

    @Test("status 의 상한 글")
    func 상한글() {
        #expect(apCheckLimitNote(minutes: 10) == " (10분 상한)")
        #expect(apCheckLimitNote(minutes: nil) == " (?분 상한)")
        #expect(apCheckLimitNote(minutes: 0.1) == " (0분 상한)", "지금 동작: 1분 아래는 0분으로 보임")
    }
}
