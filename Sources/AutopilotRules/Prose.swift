// 자율주행 규칙(AutopilotRules) — 모델이 쓴 글 다듬기와 HTML 로 옮길 때의 이스케이프(2026-10-30 3단계).
// apPlain 은 보드 Autopilot.swift 에서 옮겼고(옮기기 커밋), 같은 차례에 줄 맨 앞·끝과 붙은 빈칸을 다듬게 고쳤다.
// 보드 화면과 보고서 틀(Report.swift)이 함께 쓴다. 엔진 고침 1차에서 "모델이 결과 기록에 쓴 긴 줄표는 보고서를 그릴 때 바꾼다"로 미룬 일의 규칙이다
import Foundation

// 모델이 쓴 글의 긴 줄표는 그릴 때 쉼표로 바꾼다(설계: 보고서를 그릴 때 바꾼다, 사용자에게 보이는 글에는 긴 줄표를 쓰지 않는다).
// 줄마다: 줄 맨 앞의 줄표(목록 표지처럼 쓴 것)와 줄 끝의 줄표는 지우고, 그 밖은 앞뒤 빈칸과 함께 ", " 하나로(이어 쓴 줄표 "——" 도 하나로).
// 3단계 고침: 전에는 " — " 와 "—" 만 바꿔 "A —B" 가 "A , B", "— 앞" 이 ",  앞" 이 되었다
public func apPlain(_ s: String) -> String {
    guard s.contains("—") else { return s }
    return s.components(separatedBy: "\n").map { line -> String in
        var l = line
        l = l.replacingOccurrences(of: "^[ \\t]*—+[ \\t]*", with: "", options: .regularExpression)
        l = l.replacingOccurrences(of: "[ \\t]*—+[ \\t]*$", with: "", options: .regularExpression)
        return l.replacingOccurrences(of: "[ \\t]*—+[ \\t]*", with: ", ", options: .regularExpression)
    }.joined(separator: "\n")
}

// HTML 글·속성 값 이스케이프 — & < > " ' 를 문자 참조로. 보고서에 들어가는 모든 글(모델이 쓴 결과 기록, 엔진 기록, 판정 출력)이 이것을 거친다.
// 속성 값은 늘 큰따옴표로 감싸므로 같은 함수로 된다. 화면을 속이는 글자는 지운다: 제어 문자(탭·줄바꿈 말고), DEL·C1,
// 방향 바꾸기 글자(U+202A~202E·U+2066~2069, 보이는 차례를 뒤집는 꼴), 줄·문단 구분자(U+2028·2029)는 줄바꿈으로
public func apEscapeHTML(_ s: String) -> String {
    var o = ""
    o.reserveCapacity(s.utf8.count + 16)
    for u in s.unicodeScalars {
        switch u {
        case "&": o += "&amp;"
        case "<": o += "&lt;"
        case ">": o += "&gt;"
        case "\"": o += "&quot;"
        case "'": o += "&#39;"
        case "\n", "\t": o.unicodeScalars.append(u)
        case "\u{2028}", "\u{2029}": o += "\n"
        default:
            let v = u.value
            if v < 0x20 || (0x7F...0x9F).contains(v) || (0x202A...0x202E).contains(v) || (0x2066...0x2069).contains(v) { continue }
            o.unicodeScalars.append(u)
        }
    }
    return o
}

// 보고서에 링크로 걸 주소인가 — https 만, 호스트가 있고, 사용자 정보(아이디·암호)가 없고, 빈칸·따옴표·꺾쇠·역슬래시 같은
// 주소에 올 수 없는 글자가 없을 때만 그 글자 그대로 돌려준다(아니면 nil 이고 보고서는 글로만 보인다). javascript:·data:·file:·http:·상대 주소는 모두 nil
public func apSafeLink(_ s: String) -> String? {
    let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !t.isEmpty, t.count <= 2000 else { return nil }
    let bad: Set<Unicode.Scalar> = ["\"", "'", "<", ">", "\\", "`", "{", "}", "|", "^"]
    guard t.unicodeScalars.allSatisfy({ $0.value > 0x20 && $0.value < 0x7F && !bad.contains($0) }) else { return nil }
    guard t.lowercased().hasPrefix("https://"), let c = URLComponents(string: t), c.scheme?.lowercased() == "https",
          let host = c.host, !host.isEmpty, c.user == nil, c.password == nil else { return nil }
    return t
}
