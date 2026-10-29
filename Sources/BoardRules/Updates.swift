// 업데이트 판단 — 최신 릴리스의 VERSION 본문 검증, 이 앱보다 새 버전인지, 확인하는 빌드인지, 확인 결과
// (Updater.check 에 있던 판단을 옮김, 동작은 그대로, 이슈 #9 3차에 고친 것: 자릿수가 다른 버전은 모자란 자리를 0 으로 채워 견준다).
// 바깥(최신 VERSION 받기)은 포트(ReleaseVersions)로만 부른다.
// 앱(Updater)에 남는 것: 1시간 타이머·설정 autoUpdateCheck 읽기·손으로 누른 확인이 도는 중(checking)·알림 줄 표시와 4초 뒤 숨기기·애니메이션,
// 업데이트 실행(install.sh·종료 확인·로그 파일)
import Foundation

/// 확인하는 빌드인가 — 버전이 비었거나 build-board.sh 로 만든 개발용 빌드(1.0)면 확인하지 않는다(배포판으로 덮어쓰지 않게)
public func updateEligible(_ current: String) -> Bool { !current.isEmpty && current != "1.0" }

/// 최신 릴리스 VERSION 응답에서 버전 — 상태 200·UTF-8·앞뒤 공백과 줄바꿈을 자른 뒤 숫자(.숫자)… 꼴일 때만. 아니면 nil(확인하지 못함)
public func releaseVersion(status: Int?, body: Data?) -> String? {
    guard status == 200, let body,
          let s = String(data: body, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
          s.range(of: "^[0-9]+(\\.[0-9]+)*$", options: .regularExpression) != nil else { return nil }
    return s
}

/// 이 앱보다 새 버전인가 — 점으로 나눈 자리마다 숫자로 견준다(3.10.0 > 3.9.9). 자릿수가 다르면 모자란 쪽을 0 으로 채운다
/// (3.2 == 3.2.0, 3.2.1 > 3.2). 자리는 글자 그대로 숫자 견주기(.numeric)라 앞의 0 은 같은 수이고 Int 를 넘는 자리도 견준다
public func isNewerVersion(_ v: String, than current: String) -> Bool {
    let a = v.split(separator: ".", omittingEmptySubsequences: false)
    let b = current.split(separator: ".", omittingEmptySubsequences: false)
    for i in 0..<max(a.count, b.count) {
        let x = i < a.count ? String(a[i]) : "0", y = i < b.count ? String(b[i]) : "0"
        switch x.compare(y, options: .numeric) {
        case .orderedDescending: return true
        case .orderedAscending: return false
        case .orderedSame: continue
        }
    }
    return false
}

/// 업데이트 확인 한 번의 결과
public enum UpdateCheckResult: Equatable, Sendable {
    case devBuild          // 확인하지 않는 빌드 — 받지 않는다
    case failed            // 받지 못했거나 본문이 버전이 아님 — 알던 새 버전(latest)은 그대로 둔다
    case upToDate          // 받은 버전이 새 버전이 아님 — 알던 새 버전을 지운다
    case newer(String)     // 새 버전 — 알림 줄에 보인다

    /// 손으로 누른 확인일 때 알림 줄에 잠깐 보일 안내(SF Symbols 이름, 글자). 새 버전이면 새 버전 알림 줄이 대신 떠서 nil
    public func notice(current: String) -> (icon: String, text: String)? {
        switch self {
        case .devBuild: ("hammer.fill", "개발용 빌드(\(current))는 업데이트를 확인하지 않습니다")
        case .failed: ("exclamationmark.triangle.fill", "업데이트를 확인하지 못했습니다")
        case .upToDate: ("checkmark.circle.fill", "최신 버전입니다 (\(current))")
        case .newer: nil
        }
    }
}

/// 받은 VERSION 응답을 결과로 — 버전이 아니면 .failed, 새 버전이면 .newer, 아니면(같거나 낮음) .upToDate
public func updateCheckResult(status: Int?, body: Data?, current: String) -> UpdateCheckResult {
    guard let v = releaseVersion(status: status, body: body) else { return .failed }
    return isNewerVersion(v, than: current) ? .newer(v) : .upToDate
}

/// 유스케이스: 업데이트 확인 한 번 — 확인하지 않는 빌드면 받지 않고 .devBuild 를 바로(부른 스레드에서) 넘긴다.
/// 아니면 최신 VERSION 을 받아 가른 결과를 넘긴다(done 은 포트가 부른 큐에서 — 앱이 메인으로 옮긴다)
public func checkForUpdate(current: String, releases: ReleaseVersions, done: @escaping (UpdateCheckResult) -> Void) {
    guard updateEligible(current) else { done(.devBuild); return }
    releases.fetchLatest { status, body in done(updateCheckResult(status: status, body: body, current: current)) }
}
