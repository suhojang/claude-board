// 자율주행 규칙(AutopilotRules) — 판정 명령이 실패했을 때 엔진이 남기는 글(2026-10-30 ⑦ 확인 못 한 동작 시험에서 엔진 runUnit·check·status 에서 옮김).
// 판정 실패 뒤 다시 맡길지·미통과로 끝낼지는 apAfterCheck(Inbox.swift), 여기는 그때의 진행 기록 글·모델에게 넘기는 글·미통과 까닭·상한 글이다.
// 시간 상한(check_timeout_min, 기본 10분)에 닿아 엔진이 멈춘 판정도 실패로 보고 같은 길(다시 맡기기 → 미통과)로 가지만, 글에는 상한에 닿았다고 적는다
// (⑦ 시험: 전에는 "종료 코드 -1" 실패로만 적혀 끝나지 않는 판정과 실패한 판정이 진행 기록에서 갈리지 않았다). timedOut 은 그때의 상한(초)
import Foundation

// 상한 시간 글: 1분 아래는 초, 딱 나뉘면 분, 아니면 분과 초(시험에서 쓰는 0.1분 같은 값이 "0분"으로 보이지 않게)
public func apLimitText(sec: Double) -> String {
    let s = max(Int(sec.rounded()), 0)
    if s < 60 { return "\(s)초" }
    return s % 60 == 0 ? "\(s / 60)분" : "\(s / 60)분 \(s % 60)초"
}

// 판정 실패 뒤 다시 맡길 때의 진행 기록 글. tries 는 이미 다시 맡긴 횟수(0 이면 첫 판정), retries 는 다시 맡길 수 있는 횟수.
// 횟수가 다 찼는데 다시 맡기는 것은 그 사이 사람이 이 단위에 보낸 말이 있을 때뿐이다(apAfterCheck)
public func apCheckFailEvent(code: Int32, tries: Int, retries: Int, timedOut: Double? = nil) -> String {
    let head = timedOut.map { "판정 명령이 시간 상한(\(apLimitText(sec: $0)))에 닿아 엔진이 멈췄습니다(실패로 봅니다)." } ?? "판정 실패(종료 코드 \(code))."
    return tries < retries ? head + " 실패 출력을 모델에게 넘겨 다시 고치게 합니다(\(tries + 1)/\(retries))."
        : head + " 다시 맡길 횟수는 다 찼지만 그 사이 사람이 이 단위에 보낸 말이 있어, 그 말과 실패 출력을 넘겨 한 번 더 맡깁니다."
}

// 판정 실패 뒤 같은 세션에 넘기는 글(실패 코드·판정 명령·출력 끝부분)
public func apCheckFailMessage(code: Int32, check: String, tail: String, timedOut: Double? = nil) -> String {
    if let t = timedOut {
        return """
        판정 명령이 시간 상한(\(apLimitText(sec: t)))에 닿아 끝나지 않아 엔진이 멈췄습니다(실패로 봅니다). 판정 명령: `\(check)`
        아래는 멈출 때까지의 출력 끝부분입니다. 판정이 끝나지 않는 까닭(끝나지 않는 시험, 응답을 기다리는 서버·입력 등)을 고쳐 상한 안에 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.

        ```
        \(tail)
        ```
        """
    }
    return """
    판정 명령이 실패했습니다(종료 코드 \(code)). 판정 명령: `\(check)`
    아래는 출력 끝부분입니다. 원인을 고쳐 판정을 통과하게 하세요. 판정 명령과 시험 기준은 바꾸지 마세요. 마치면 result.json 을 고쳐 쓰세요.

    ```
    \(tail)
    ```
    """
}

// 다시 맡겨도 판정을 통과하지 못해 미통과로 끝낼 때의 까닭
// timedOut: 마지막 판정이 시간 상한에 닿아 멈췄으면 그 상한(초)
public func apCheckFailReason(tries: Int, timedOut: Double? = nil) -> String {
    "\(tries == 0 ? "" : "\(tries)번 다시 해도 ")판정 명령을 통과하지 못해" + (timedOut.map { "(마지막 판정은 시간 상한 \(apLimitText(sec: $0))에 닿아 멈춤)" } ?? "")
        + " 미통과로 기록하고 다음 단위로 갑니다. 기준은 낮추지 않습니다."
}

// 출발 전 status 의 판정 줄 끝에 붙는 상한 글(단위에 check_timeout_min 이 있을 때). minutes 가 수가 아니면 nil
public func apCheckLimitNote(minutes: Double?) -> String {
    " (\(minutes.map { apLimitText(sec: $0 * 60) } ?? "?분") 상한)"
}

// 판정·고정 단계 출력의 끝부분(2026-10-30 ⑪-1 시험 실행기에서 엔진 fixedCommand·plainCommand 에서 옮김): 마지막 4000 바이트를 UTF-8 로 풀어
// 마지막 40줄. 모델에게 넘기는 실패 글·engine.json 의 tail·prerun.json 의 tail 이 이 글이다
public func apOutputTail(_ data: Data) -> String {
    String(decoding: data.suffix(4000), as: UTF8.self)
        .split(separator: "\n", omittingEmptySubsequences: false).suffix(40).joined(separator: "\n")
}

// 판정 미리 돌리기에서 일하기 전부터 통과한 단위의 출발 카드 감(prerun.json 의 cards, ⑪-1 에서 엔진 cmdPrerun 에서 옮김)
public func apPrerunCard(n: Int, id: String, title: String) -> [String: Any] {
    ["id": "prerun-\(id)", "unit": n, "kind": "prerun",
     "title": "「\(title)」 의 판정이 일하기 전부터 통과합니다",
     "why": "이미 끝난 일이거나 판정이 약하다는 뜻입니다. 그대로 두면 모델이 일하지 않아도 통과로 기록됩니다.",
     "options": [["key": "drop", "label": "빼기"], ["key": "fix_check", "label": "판정 고치기"], ["key": "keep", "label": "그대로"]],
     "recommended": "drop"]
}
