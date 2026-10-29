// 자율주행 규칙(AutopilotRules) — 판정 명령이 실패했을 때 엔진이 남기는 글(2026-10-30 ⑦ 확인 못 한 동작 시험에서 엔진 runUnit·check·status 에서 옮김).
// 판정 실패 뒤 다시 맡길지·미통과로 끝낼지는 apAfterCheck(Inbox.swift), 여기는 그때의 진행 기록 글·모델에게 넘기는 글·미통과 까닭·상한 글이다
import Foundation

// 판정 실패 뒤 다시 맡길 때의 진행 기록 글. tries 는 이미 다시 맡긴 횟수(0 이면 첫 판정), retries 는 다시 맡길 수 있는 횟수.
// 횟수가 다 찼는데 다시 맡기는 것은 그 사이 사람이 이 단위에 보낸 말이 있을 때뿐이다(apAfterCheck)
public func apCheckFailEvent(code: Int32, tries: Int, retries: Int) -> String {
    tries < retries ? "판정 실패(종료 코드 \(code)). 실패 출력을 모델에게 넘겨 다시 고치게 합니다(\(tries + 1)/\(retries))."
        : "판정 실패(종료 코드 \(code)). 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다."
}

// 판정 실패 뒤 같은 세션에 넘기는 글(실패 코드·판정 명령·출력 끝부분)
public func apCheckFailMessage(code: Int32, check: String, tail: String) -> String {
    """
    판정 명령이 실패했습니다(종료 코드 \(code)). 판정 명령: `\(check)`
    아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.

    ```
    \(tail)
    ```
    """
}

// 다시 맡겨도 판정을 통과하지 못해 미통과로 끝낼 때의 까닭
public func apCheckFailReason(tries: Int) -> String {
    "\(tries == 0 ? "" : "\(tries)번 다시 해도 ")판정 명령을 통과하지 못해 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다."
}

// 출발 전 status 의 판정 줄 끝에 붙는 상한 글(단위에 check_timeout_min 이 있을 때). minutes 가 수가 아니면 nil
public func apCheckLimitNote(minutes: Double?) -> String {
    " (\(minutes.map { String(Int($0)) } ?? "?")분 상한)"
}
