// Claude Code 세션 보드 — 작업 경로별 카드로 세션 상태를 보여 주는 네이티브 창
// 상태 파일: ~/.claude/notifier/sessions/*.json (state.py 가 hook 에서 기록)
import AppKit
import BoardRules
import Carbon.HIToolbox
import Combine
import CryptoKit
import GhosttyTerminal
import GhosttyTheme
import SQLite3
import SwiftUI
import WebKit

// MARK: - 브레인롯 캐릭터

// 제목 오른쪽 캐릭터 — 브레인롯 7종과 기존 현장 감독. 고른 값은 "mascot" 에 남아 다시 켜도 유지된다
enum Mascot: String, CaseIterable, Identifiable {
    case tralalero, bombardiro, tungtung, lirili, patapim, boneca, cappuccino, foreman
    var id: String { rawValue }

    var name: String {
        switch self {
        case .tralalero: "트랄랄레로 트랄랄라"
        case .bombardiro: "봄바르디로 크로코딜로"
        case .tungtung: "퉁 퉁 퉁 사후르"
        case .lirili: "리릴리 라릴라"
        case .patapim: "브르르 브르르 파타핌"
        case .boneca: "보네카 암발라부"
        case .cappuccino: "카푸치노 아사시노"
        case .foreman: "현장 감독"
        }
    }

    // 말풍선에 가끔 끼우는 유행어 — 감독은 없다
    var catchphrase: String? {
        switch self {
        case .tralalero: "트랄랄레로 트랄랄라~"
        case .bombardiro: "봄바르디로 크로코딜로!"
        case .tungtung: "퉁 퉁 퉁 퉁 사후르!"
        case .lirili: "리릴리 라릴라~"
        case .patapim: "브르르 브르르 파타핌!"
        case .boneca: "보네카 암발라부!"
        case .cappuccino: "카푸치노 아사시노!"
        case .foreman: nil
        }
    }
}

private let ink = Color(white: 0.12)

// 0~1 비율 좌표의 꼭짓점을 이은 다각형 — 지느러미·이빨·꼬리 같은 작은 조각
struct Poly: Shape {
    let pts: [(CGFloat, CGFloat)]
    init(_ pts: [(CGFloat, CGFloat)]) { self.pts = pts }
    func path(in r: CGRect) -> Path {
        Path { p in
            p.addLines(pts.map { CGPoint(x: r.minX + $0.0 * r.width, y: r.minY + $0.1 * r.height) })
            p.closeSubpath()
        }
    }
}

// 만화 눈 — 흰자에 눈동자. 졸리거나 깜빡이면 감고, 재촉할 땐 눈동자가 작아진다(놀란 눈). 눈동자는 말풍선 쪽(왼쪽)을 본다
struct CartoonEye: View {
    let size: CGFloat
    let mood: Mood
    let blink: Bool
    var body: some View {
        if mood == .sleepy || blink {
            Capsule().fill(ink).frame(width: size, height: 1.2)
        } else {
            Circle().fill(Color.white).frame(width: size, height: size)
                .overlay(Circle().fill(ink).frame(width: size * (mood == .urgent ? 0.42 : 0.56)).offset(x: -size * 0.12))
        }
    }
}

// 아래로 뾰족한 흰 이빨 줄
struct Teeth: View {
    let n: Int
    let width: CGFloat
    let height: CGFloat
    var body: some View {
        HStack(spacing: 0) {
            ForEach(0..<n, id: \.self) { _ in
                Poly([(0, 0), (1, 0), (0.5, 1)]).fill(Color.white).frame(width: width / CGFloat(n), height: height)
            }
        }
    }
}

// 운동화 — 흰 신발에 빨간 곡선, 앞코가 왼쪽
struct Sneaker: View {
    var body: some View {
        RoundedRectangle(cornerRadius: 1.5).fill(Color.white).frame(width: 7, height: 3.5)
            .overlay(Path { p in
                p.move(to: CGPoint(x: 1.5, y: 1.4))
                p.addQuadCurve(to: CGPoint(x: 6, y: 0.8), control: CGPoint(x: 3, y: 3.3))
            }.stroke(Color(red: 0.95, green: 0.30, blue: 0.25), lineWidth: 0.8))
            .offset(x: -1.5)
    }
}

// 브레인롯 캐릭터 그림과 동작 — 감독처럼 모두 공유 시계 t 의 함수다.
// 말풍선이 바뀌면 폴짝 뛰고, 기분에 따라 박자(beat)가 빨라지거나 느려진다. 머리 위 "!"·"z" 와 크기(46×44)는 감독과 같다
struct BrainrotFigure: View {
    let kind: Mascot
    let mood: Mood
    let t: TimeInterval
    let sinceChange: Double
    let blink: Bool

    // 기분별 박자 — 재촉 0.3초 · 작업 0.6초 · 대기 1.6초 · 졸음 3초에 한 번
    private var period: Double {
        switch mood { case .urgent: 0.3; case .busy: 0.6; case .idle: 1.6; case .sleepy: 3.0 }
    }
    private var beat: Double { cycle(t, period) }          // 0 → 1
    private var wave: Double { sin(2 * .pi * beat) }       // -1 ~ 1

    var body: some View {
        // 말풍선이 바뀌면 폴짝 — 0.45초 동안 뛰었다 내려와 착지에 찌그러진다(감독과 같은 곡선)
        let hopP = min(max(sinceChange / 0.45, 0), 1)
        var hop = sinceChange < 0.45 ? -9 * sin(.pi * hopP) : 0
        var land = sinceChange >= 0.45 && sinceChange < 0.62 ? sin(.pi * (sinceChange - 0.45) / 0.17) : 0
        var tilt = 0.0, dx = 0.0, breathe = 0.0
        switch mood {
        case .urgent:
            let p = cycle(t, 0.55)
            hop = min(hop, -6 * sin(.pi * p))
            land = max(land, p > 0.85 ? sin(.pi * (p - 0.85) / 0.15) : 0)
            dx = 1.5 * sin(2 * .pi * t / 0.12)
        case .busy:
            tilt = 5 * sin(2 * .pi * t / 0.9)
            hop = min(hop, -2 * abs(sin(2 * .pi * t / 0.9)))
        case .idle:
            tilt = 4 * sin(2 * .pi * t / 2.4)
            breathe = swing(t, 1.6)
        case .sleepy:
            tilt = 8 * sin(2 * .pi * t / 3.2)
            breathe = swing(t, 3.2)
        }
        let squash = land * 0.16
        return ZStack {
            character
            if mood == .urgent {
                Text("!").font(.system(size: 13, weight: .black)).foregroundStyle(Color.yellow)
                    .scaleEffect(1 + 0.25 * swing(t, 0.4))
                    .offset(x: 14, y: -19)
            } else if mood == .sleepy {
                ForEach(0..<2, id: \.self) { i in
                    let p = cycle(t + Double(i) * 1.2, 2.4)
                    Text("z").font(.system(size: 8 + 3 * p, weight: .bold)).foregroundStyle(.white.opacity(1 - p))
                        .offset(x: 12 + 6 * p, y: -10 - 14 * p)
                }
            }
        }
        .frame(width: 46, height: 44)
        .scaleEffect(x: 1 + squash + 0.03 * breathe, y: 1 - squash + 0.03 * breathe, anchor: .bottom)
        .rotationEffect(.degrees(tilt), anchor: .bottom)
        .offset(x: dx, y: hop + 2)
    }

    @ViewBuilder private var character: some View {
        switch kind {
        case .tralalero: tralalero
        case .bombardiro: bombardiro
        case .tungtung: tungtung
        case .lirili: lirili
        case .patapim: patapim
        case .boneca: boneca
        case .cappuccino: cappuccino
        case .foreman: EmptyView()   // 감독은 Foreman.figure 가 그린다
        }
    }

    // 트랄랄레로 트랄랄라 — 파란 상어가 세 다리에 운동화를 신고 걷는다. 재촉하면 제자리 뛰기, 졸리면 멈춘다
    private var tralalero: some View {
        let blue = Color(red: 0.36, green: 0.58, blue: 0.86), belly = Color(red: 0.90, green: 0.95, blue: 1.0)
        let lift: Double = mood == .sleepy ? 0 : mood == .idle ? 1.5 : 3
        return ZStack {
            ForEach(0..<3, id: \.self) { i in
                let up = max(0, sin(2 * .pi * beat + Double(i) * 2.1))
                VStack(spacing: -1) {
                    Capsule().fill(blue).frame(width: 2.6, height: 8)
                    Sneaker()
                }
                .offset(x: Double(i - 1) * 7 - 2, y: 13 - lift * up)
            }
            // 꼬리 — 박자에 맞춰 흔든다
            Poly([(0, 0.5), (1, 0), (0.7, 0.5), (1, 1)]).fill(blue)
                .frame(width: 10, height: 13)
                .rotationEffect(.degrees(12 * wave), anchor: .leading)
                .offset(x: 18, y: -5)
            Poly([(0, 1), (0.55, 0), (1, 1)]).fill(blue).frame(width: 9, height: 8).offset(x: 2, y: -14)   // 등지느러미
            Capsule().fill(blue).frame(width: 32, height: 15).offset(y: -4)
            Capsule().fill(belly).frame(width: 22, height: 6).offset(x: -3, y: 0.5)
            HStack(spacing: 1.5) {   // 아가미
                ForEach(0..<3, id: \.self) { _ in Capsule().fill(ink.opacity(0.35)).frame(width: 0.8, height: 5) }
            }
            .offset(x: 1, y: -5)
            CartoonEye(size: 5.5, mood: mood, blink: blink).offset(x: -9, y: -7)
            Capsule().fill(ink).frame(width: 9, height: 1.2).offset(x: -11, y: -1.5)   // 입
            Teeth(n: 4, width: 8, height: 2.5).offset(x: -11, y: 0.3)
        }
    }

    // 봄바르디로 크로코딜로 — 악어 머리를 단 폭격기. 프로펠러가 돌고, 재촉하면 폭탄을 떨어뜨린다
    private var bombardiro: some View {
        let green = Color(red: 0.38, green: 0.64, blue: 0.34), dark = Color(red: 0.26, green: 0.46, blue: 0.24)
        let metal = Color(red: 0.62, green: 0.66, blue: 0.70)
        let spin = abs(cos(2 * .pi * t / (mood == .sleepy ? 1.2 : mood == .idle ? 0.25 : 0.1)))
        let drop = cycle(t, 0.6)
        return ZStack {
            if mood == .urgent {
                Capsule().fill(Color(white: 0.22)).frame(width: 4, height: 6)
                    .offset(x: 4, y: 6 + 16 * drop)
                    .opacity(1 - drop)
            }
            Poly([(0.3, 1), (0.8, 1), (1, 0), (0.6, 0)]).fill(metal.opacity(0.7))   // 먼 쪽 날개 — 몸 뒤로 비스듬히
                .frame(width: 18, height: 8).offset(x: 7, y: -9)
            Poly([(0, 1), (0.35, 0), (1, 0), (0.9, 1)]).fill(metal).frame(width: 8, height: 9).offset(x: 17, y: -8)   // 꼬리날개
            Capsule().fill(green).frame(width: 30, height: 12).offset(x: 3)
            HStack(spacing: 2) {   // 등 돌기
                ForEach(0..<3, id: \.self) { _ in Poly([(0, 1), (0.5, 0), (1, 1)]).fill(dark).frame(width: 2.5, height: 2) }
            }
            .offset(x: 9, y: -6.5)
            RoundedRectangle(cornerRadius: 3).fill(green).frame(width: 15, height: 6).offset(x: -15, y: 2)   // 주둥이
            Capsule().fill(dark).frame(width: 12, height: 1).offset(x: -15, y: 3)
            Teeth(n: 4, width: 10, height: 2).offset(x: -15, y: 4.5)
            Circle().fill(dark).frame(width: 1.5, height: 1.5).offset(x: -21, y: 0)   // 콧구멍
            Circle().fill(green).frame(width: 8, height: 8)   // 툭 튀어나온 눈두덩
                .overlay(CartoonEye(size: 5, mood: mood, blink: blink))
                .offset(x: -8, y: -5)
            Poly([(0.15, 0), (0.75, 0), (0.6, 1), (0, 1)]).fill(metal)   // 가까운 쪽 날개 — 몸 아래 앞으로
                .frame(width: 22, height: 8).offset(x: 3, y: 8)
            Capsule().fill(Color(white: 0.35)).frame(width: 7, height: 4.5).offset(x: -3, y: 10)   // 날개 위 엔진
            Capsule().fill(Color(white: 0.9).opacity(0.85)).frame(width: 2, height: 12)   // 프로펠러 — 길이가 줄었다 늘며 도는 것처럼 보인다
                .scaleEffect(y: 0.15 + 0.85 * spin)
                .offset(x: -7.5, y: 10)
        }
    }

    // 퉁 퉁 퉁 사후르 — 통나무 몸에 야구방망이. 박자에 맞춰 방망이로 바닥을 퉁퉁 내려친다
    private var tungtung: some View {
        let wood = Color(red: 0.78, green: 0.57, blue: 0.35), top = Color(red: 0.92, green: 0.76, blue: 0.52)
        let bark = Color(red: 0.52, green: 0.34, blue: 0.19), batC = Color(red: 0.95, green: 0.84, blue: 0.62)
        // 바닥을 치는 정도(0~1) — 박자 가운데서 짧게 내려친다. 졸리면 방망이를 내려놓는다
        let hit = mood == .sleepy ? 0 : pow(abs(sin(.pi * beat)), mood == .urgent ? 2 : 4)
        let angle = mood == .sleepy ? 160 : 20 + 130 * hit
        return ZStack {
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 3, height: 7) } }
                .offset(x: -4, y: 18)
            RoundedRectangle(cornerRadius: 5).fill(wood).frame(width: 20, height: 30).offset(x: -4, y: 1)
            HStack(spacing: 4) {   // 나뭇결
                ForEach(0..<3, id: \.self) { _ in Capsule().fill(bark.opacity(0.25)).frame(width: 0.8, height: 22) }
            }
            .offset(x: -4, y: 4)
            Ellipse().fill(top).frame(width: 20, height: 6)   // 윗면 나이테
                .overlay(Ellipse().stroke(bark.opacity(0.45), lineWidth: 0.6).frame(width: 11, height: 3))
                .offset(x: -4, y: -14)
            HStack(spacing: 3) {
                CartoonEye(size: 6.5, mood: mood, blink: blink)
                CartoonEye(size: 6.5, mood: mood, blink: blink)
            }
            .offset(x: -5, y: -6)
            if mood == .urgent {
                Ellipse().fill(ink).frame(width: 7, height: 3 + 3 * swing(t, 0.3)).offset(x: -5, y: 4)
            } else {
                Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 10, y: 0), control: CGPoint(x: 5, y: 5)) }
                    .stroke(ink, style: StrokeStyle(lineWidth: 1.4, lineCap: .round))
                    .frame(width: 10, height: 5)
                    .offset(x: -5, y: 5)
            }
            // 방망이 — 아래 절반을 비워 손이 회전 중심이 되게 한다
            Poly([(0.3, 1), (0, 0.06), (0.5, 0), (1, 0.06), (0.7, 1)]).fill(batC)
                .frame(width: 5, height: 17)
                .frame(width: 5, height: 34, alignment: .top)
                .rotationEffect(.degrees(angle))
                .offset(x: 8, y: 2)
            Circle().fill(bark).frame(width: 4, height: 4).offset(x: 8, y: 2)   // 손
            if hit > 0.7 {
                Text("퉁").font(.system(size: 7, weight: .black)).foregroundStyle(Color.yellow)
                    .opacity((hit - 0.7) / 0.3)
                    .offset(x: 19, y: 19)
            }
        }
    }

    // 리릴리 라릴라 — 코끼리 머리에 선인장 몸, 샌들, 자명종. 귀가 펄럭이고 코가 흔들리며, 재촉하면 자명종이 울린다
    private var lirili: some View {
        let cactus = Color(red: 0.36, green: 0.68, blue: 0.40), spine = Color(red: 0.86, green: 0.95, blue: 0.80)
        let grey = Color(red: 0.64, green: 0.66, blue: 0.72), pink = Color(red: 0.95, green: 0.72, blue: 0.76)
        let sandal = Color(red: 0.62, green: 0.40, blue: 0.22), bell = Color(red: 0.92, green: 0.30, blue: 0.28)
        let sway = (mood == .sleepy ? 4 : mood == .urgent ? 22 : 14) * wave
        let ring = mood == .urgent ? 14 * sin(2 * .pi * t / 0.08) : 0
        let spines: [(Double, Double)] = [(-3, 4), (2, 7), (-2, 12), (3, 14), (-10, 2)]
        return ZStack {
            HStack(spacing: 4) {   // 샌들 신은 발
                ForEach(0..<2, id: \.self) { _ in
                    VStack(spacing: 0) {
                        Capsule().fill(cactus).frame(width: 3, height: 5)
                        Capsule().fill(sandal).frame(width: 7, height: 2)
                    }
                }
            }
            .offset(y: 19)
            Capsule().fill(cactus).frame(width: 13, height: 20).offset(y: 8)   // 선인장 몸통
            Capsule().fill(cactus).frame(width: 4, height: 9).offset(x: -10, y: 4)   // 왼쪽 가지
            Capsule().fill(cactus).frame(width: 6, height: 3.5).offset(x: -8, y: 8)
            Capsule().fill(cactus).frame(width: 7, height: 3.5).offset(x: 8, y: 7)   // 자명종 든 오른쪽 가지
            ForEach(0..<spines.count, id: \.self) { i in
                Circle().fill(spine).frame(width: 1.2, height: 1.2).offset(x: spines[i].0, y: spines[i].1)
            }
            Ellipse().fill(grey).frame(width: 10, height: 14)   // 귀 — 펄럭인다
                .overlay(Ellipse().fill(pink).frame(width: 6, height: 9))
                .scaleEffect(x: 1 + 0.12 * wave, anchor: .trailing)
                .offset(x: -10, y: -10)
            Ellipse().fill(grey).frame(width: 10, height: 14)
                .overlay(Ellipse().fill(pink).frame(width: 6, height: 9))
                .scaleEffect(x: 1 + 0.12 * wave, anchor: .leading)
                .offset(x: 10, y: -10)
            Circle().fill(grey).frame(width: 18, height: 18).offset(y: -11)
            HStack(spacing: 5) {
                CartoonEye(size: 4.5, mood: mood, blink: blink)
                CartoonEye(size: 4.5, mood: mood, blink: blink)
            }
            .offset(y: -13)
            // 코 — 위 끝을 축으로 흔들린다(아래 절반을 비워 회전 중심을 위로)
            Capsule().fill(grey).frame(width: 5, height: 13)
                .frame(width: 5, height: 26, alignment: .bottom)
                .rotationEffect(.degrees(sway))
                .offset(y: -6)
            ZStack {   // 자명종
                HStack(spacing: 4) { Circle().fill(bell).frame(width: 4, height: 4); Circle().fill(bell).frame(width: 4, height: 4) }
                    .offset(y: -4.5)
                Circle().fill(Color.white).frame(width: 9, height: 9).overlay(Circle().stroke(bell, lineWidth: 1.2))
                Capsule().fill(ink).frame(width: 0.9, height: 3).offset(y: -1.2)
                Capsule().fill(ink).frame(width: 2.5, height: 0.9).offset(x: 1)
            }
            .rotationEffect(.degrees(ring))
            .offset(x: 13, y: 4)
        }
    }

    // 브르르 브르르 파타핌 — 큰 코의 원숭이 머리에 나무 몸. 잎이 살랑이고 코가 출렁이며, 재촉하면 부르르 떤다
    private var patapim: some View {
        let fur = Color(red: 0.72, green: 0.46, blue: 0.26), face = Color(red: 0.96, green: 0.82, blue: 0.64)
        let nose = Color(red: 0.94, green: 0.60, blue: 0.46), bark = Color(red: 0.50, green: 0.33, blue: 0.20)
        let leaf = Color(red: 0.42, green: 0.72, blue: 0.36)
        let shake = mood == .urgent ? 1.2 * sin(2 * .pi * t / 0.05) : 0
        let leaves: [(Double, Double)] = [(-10, 3), (-7, -1), (10, 3), (7, -1)]
        return ZStack {
            HStack(spacing: 6) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 8, height: 3.5) } }   // 큰 발
                .offset(y: 20)
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(bark).frame(width: 3, height: 8) } }
                .offset(y: 15)
            RoundedRectangle(cornerRadius: 4).fill(bark).frame(width: 13, height: 14).offset(y: 6)   // 나무 몸통
            ForEach(0..<leaves.count, id: \.self) { i in
                Circle().fill(leaf).frame(width: 7, height: 7)
                    .offset(x: leaves[i].0, y: leaves[i].1 + 0.8 * wave * (i % 2 == 0 ? 1 : -1))
            }
            Circle().fill(fur).frame(width: 22, height: 22).offset(y: -9)
            Ellipse().fill(face).frame(width: 16, height: 14).offset(y: -7)
            HStack(spacing: 6) {
                CartoonEye(size: 4, mood: mood, blink: blink)
                CartoonEye(size: 4, mood: mood, blink: blink)
            }
            .offset(y: -11)
            Ellipse().fill(nose).frame(width: 9, height: 10)   // 큰 코 — 아래로 늘어져 출렁인다
                .scaleEffect(y: 1 + 0.08 * wave, anchor: .top)
                .offset(y: -2)
        }
        .offset(x: shake)
    }

    // 보네카 암발라부 — 개구리 머리, 타이어 몸통, 사람 다리. 타이어가 구르며 걷고, 재촉하면 입을 벌린다
    private var boneca: some View {
        let frog = Color(red: 0.44, green: 0.74, blue: 0.34), frogDark = Color(red: 0.30, green: 0.55, blue: 0.24)
        let skin = Color(red: 0.93, green: 0.74, blue: 0.58)
        let roll = mood == .sleepy ? 0 : t / period * 90
        return ZStack {
            ForEach(0..<2, id: \.self) { i in   // 사람 다리 — 번갈아 든다
                VStack(spacing: -1) {
                    Capsule().fill(skin).frame(width: 3, height: 9)
                    Capsule().fill(ink).frame(width: 6, height: 2.5).offset(x: -1.5)
                }
                .offset(x: Double(i) * 7 - 3.5, y: 16 - (mood == .sleepy ? 0 : 2 * max(0, sin(2 * .pi * beat + Double(i) * .pi))))
            }
            ZStack {   // 타이어 몸통 — 굴러간다
                Circle().fill(Color(white: 0.17))
                ForEach(0..<10, id: \.self) { k in
                    Rectangle().fill(Color(white: 0.32)).frame(width: 2, height: 3)
                        .offset(y: -8.5).rotationEffect(.degrees(Double(k) * 36))
                }
                Circle().fill(Color(white: 0.55)).frame(width: 8, height: 8)
                Circle().fill(Color(white: 0.30)).frame(width: 3, height: 3)
            }
            .frame(width: 20, height: 20)
            .rotationEffect(.degrees(roll))
            .offset(y: 4)
            Ellipse().fill(frog).frame(width: 26, height: 13).offset(y: -9)   // 개구리 머리
            ForEach([-7.0, 7.0], id: \.self) { x in   // 툭 튀어나온 눈
                Circle().fill(frog).frame(width: 9, height: 9)
                    .overlay(CartoonEye(size: 6, mood: mood, blink: blink))
                    .offset(x: x, y: -15)
            }
            Circle().fill(frogDark).frame(width: 2, height: 2).offset(x: -9, y: -8)   // 점
            Circle().fill(frogDark).frame(width: 1.5, height: 1.5).offset(x: 9, y: -7)
            if mood == .urgent {
                Ellipse().fill(Color(red: 0.55, green: 0.15, blue: 0.20)).frame(width: 12, height: 3 + 2 * swing(t, 0.3)).offset(y: -6)
            } else {
                Path { p in p.move(to: CGPoint(x: 0, y: 0)); p.addQuadCurve(to: CGPoint(x: 16, y: 0), control: CGPoint(x: 8, y: 3)) }
                    .stroke(frogDark, style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
                    .frame(width: 16, height: 3)
                    .offset(y: -6)
            }
        }
    }

    // 카푸치노 아사시노 — 닌자 복면을 쓴 카푸치노 잔. 카타나를 휘두르고, 바쁠 땐 속도선을 남기며 오간다
    private var cappuccino: some View {
        let cup = Color(white: 0.96), coffee = Color(red: 0.55, green: 0.36, blue: 0.22)
        let foam = Color(red: 0.97, green: 0.89, blue: 0.76), band = Color(red: 0.88, green: 0.22, blue: 0.24)
        let slash: Double = switch mood {
        case .urgent: -70 + 110 * swing(t, 0.25)
        case .busy: -40 + 60 * swing(t, 0.6)
        case .idle: -35 + 6 * wave
        case .sleepy: 40
        }
        let dash = mood == .busy ? 3 * sin(2 * .pi * t / 0.6) : 0
        let flutter = 18 * sin(2 * .pi * t / (mood == .sleepy ? 2.0 : 0.35))
        return ZStack {
            if mood == .busy || mood == .urgent {   // 속도선
                ForEach(0..<3, id: \.self) { i in
                    Capsule().fill(Color.white.opacity(0.35)).frame(width: 6 + 3 * Double(i % 2), height: 1)
                        .offset(x: 20, y: Double(i) * 5 - 4)
                }
            }
            HStack(spacing: 5) { ForEach(0..<2, id: \.self) { _ in Capsule().fill(ink).frame(width: 2.5, height: 7) } }   // 다리
                .offset(y: 18)
            ForEach(0..<2, id: \.self) { i in   // 머리띠 꼬리 — 바람에 날린다
                Capsule().fill(band).frame(width: 8, height: 2)
                    .rotationEffect(.degrees(flutter + Double(i) * 25 - 10), anchor: .leading)
                    .offset(x: 14, y: -9 + Double(i) * 2)
            }
            Circle().stroke(cup, lineWidth: 2.5).frame(width: 8, height: 9).offset(x: 11, y: 4)   // 손잡이
            UnevenRoundedRectangle(topLeadingRadius: 2, bottomLeadingRadius: 7, bottomTrailingRadius: 7, topTrailingRadius: 2)
                .fill(cup).frame(width: 22, height: 21).offset(y: 3)
            Ellipse().fill(coffee).frame(width: 22, height: 6).offset(y: -7.5)
            Ellipse().fill(foam).frame(width: 15, height: 3.5).offset(y: -7.5)
            Rectangle().fill(Color(white: 0.14)).frame(width: 22, height: 7).offset(y: -1)   // 닌자 복면 — 눈만 드러난다
            Rectangle().fill(band).frame(width: 22, height: 1.5).offset(y: -4.5)
            HStack(spacing: 4) {
                CartoonEye(size: 4.5, mood: mood, blink: blink)
                CartoonEye(size: 4.5, mood: mood, blink: blink)
            }
            .offset(y: -1)
            Capsule().fill(ink).frame(width: 5, height: 2).offset(x: -12, y: 6)   // 팔
            // 카타나 — 손잡이 가운데(손)가 회전 중심이 되게 위로 붙여 둔다
            VStack(spacing: 0) {
                Capsule().fill(Color(white: 0.88)).frame(width: 2, height: 16)
                Capsule().fill(Color(red: 0.85, green: 0.70, blue: 0.30)).frame(width: 6, height: 1.6)
                Capsule().fill(Color(white: 0.15)).frame(width: 2.4, height: 6)
            }
            .frame(width: 6, height: 41, alignment: .top)
            .rotationEffect(.degrees(slash))
            .offset(x: -14, y: 6)
        }
        .offset(x: dash)
    }
}

// viewport = 목록 칸(스크롤 또는 빈 화면 안내) 높이, board = 보드 내용 전체 높이 — 창 크기 단계를 고를 때 쓴다
final class CardTopsBox { var values: [String: CGFloat] = [:]; var viewport: CGFloat = 0; var board: CGFloat = 0 }

// 오른쪽 독의 터미널 열기 단추 — 설치된 앱은 그 앱 아이콘으로 열 수 있고, 없으면 흐리게 막아 둔다
struct TerminalApp: Identifiable {
    let id: String      // 번들 ID
    let name: String
}
let terminalApps = [TerminalApp(id: "com.mitchellh.ghostty", name: "Ghostty"),
                    TerminalApp(id: "com.googlecode.iterm2", name: "iTerm2"),
                    TerminalApp(id: "com.apple.Terminal", name: "터미널"),
                    TerminalApp(id: "com.stablyai.orca", name: "Orca")]

// 명령줄 도구를 찾는 곳 — 보드는 로그인 셸의 PATH 를 모르고 실행되므로 흔한 설치 위치를 직접 본다
func findCommand(_ name: String) -> URL? {
    let home = NSHomeDirectory()
    let dirs = ["\(home)/.local/bin", "\(home)/.\(name)/bin", "\(home)/.cargo/bin", "\(home)/bin",
                "/opt/homebrew/bin", "/usr/local/bin"]
    return dirs.map { URL(fileURLWithPath: "\($0)/\(name)") }
        .first { FileManager.default.isExecutableFile(atPath: $0.path) }
}

final class TerminalAppStore: ObservableObject {
    static let shared = TerminalAppStore()
    @Published private(set) var urls: [String: URL] = [:]       // 설치된 앱 위치
    @Published private(set) var running: Set<String> = []
    // 앱 아이콘 — iconForFile 은 느려서 화면을 그릴 때마다 부르면 보드가 멈칫한다. 설치 위치가 바뀔 때만 다시 읽는다
    private(set) var icons: [String: NSImage] = [:]
    private var timer: Timer?
    private init() {
        refresh()
        // 설치·삭제는 드물어 1분마다, 실행·종료는 알림으로 바로 반영한다
        timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in self?.refresh() }
        let nc = NSWorkspace.shared.notificationCenter
        for n in [NSWorkspace.didLaunchApplicationNotification, NSWorkspace.didTerminateApplicationNotification] {
            nc.addObserver(forName: n, object: nil, queue: .main) { [weak self] _ in self?.refreshRunning() }
        }
    }
    func refresh() {
        var u: [String: URL] = [:]
        for a in terminalApps {
            u[a.id] = NSWorkspace.shared.urlForApplication(withBundleIdentifier: a.id)
        }
        if u != urls {
            var ic: [String: NSImage] = [:]
            for a in terminalApps where u[a.id] != nil {
                ic[a.id] = NSWorkspace.shared.icon(forFile: u[a.id]!.path)
            }
            icons = ic
            urls = u
        }
        refreshRunning()
    }
    private func refreshRunning() {
        let r = Set(NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier)).intersection(terminalApps.map(\.id))
        if r != running { running = r }
    }
    func open(_ id: String) {
        guard let url = urls[id] else { return }
        let cfg = NSWorkspace.OpenConfiguration()
        cfg.activates = true
        NSWorkspace.shared.openApplication(at: url, configuration: cfg)
    }
}

func shellQuote(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

// 사용자의 로그인 셸(passwd 의 셸) — 없으면 zsh
func userShell() -> String {
    if let pw = getpwuid(getuid()), let sh = pw.pointee.pw_shell, case let path = String(cString: sh), !path.isEmpty { return path }
    return "/bin/zsh"
}
// 오른쪽 아래 독: Ghostty·iTerm2·터미널·Orca 를 바로 연다.
// 평소엔 창 오른쪽 아래 끝의 탭만 보이다가 그 탭(창 오른쪽 끝 아래쪽)에 마우스를 대면 보드 위로 미끄러져 나오고, 독에서 벗어나면 다시 숨는다.
// 보드 위치가 왼쪽이면 보드의 왼쪽 끝(화면 바깥쪽)에 거울처럼 붙는다 — 오른쪽 끝은 터미널과의 경계라 경계를 잡으러 가다 열리지 않게
struct EdgeDock: View {
    static let size: CGFloat = 36, gap: CGFloat = 8, pad: CGFloat = 8
    static let trigger: CGFloat = 16    // 창 오른쪽 끝에서 이 폭(탭 폭) 안에 마우스가 오면 연다
    static let bottomInset: CGFloat = 34    // 창 아래 끝과 독 사이 — 오른쪽 아래 투명도 슬라이더를 가리지 않게
    static let hintHeight: CGFloat = 26
    static var width: CGFloat { size + pad * 2 }
    static var height: CGFloat { CGFloat(terminalApps.count) * (size + gap) - gap + pad * 2 }
    // 오른쪽 끝에 마우스를 대면 열리는 높이 — 창 아래에서 독 윗변 조금 위까지만. 위쪽 끝에서는 열지 않는다
    static var triggerHeight: CGFloat { bottomInset + height + 12 }
    @ObservedObject private var pointer = PointerTracker.shared
    @ObservedObject private var store = TerminalAppStore.shared
    @ObservedObject private var hub = TerminalHub.shared
    @State private var open = false
    @State private var overPanel = false
    @State private var closing: DispatchWorkItem?
    @State private var hovered: Int?        // 마우스가 올라간 아이콘 순서

    // 오른쪽 끝이나 독 위에 있으면 연다. 벗어나면 조금 기다렸다 닫는다 — 끝에서 독으로 옮겨 가는 사이에 닫히지 않게
    private func sync() {
        closing?.cancel()
        if pointer.atEdge || overPanel {
            if !open { open = true }
        } else if open {
            let w = DispatchWorkItem { open = false; hovered = nil }
            closing = w
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: w)
        }
    }

    private func index(at y: CGFloat) -> Int? {
        let k = Int(((y - Self.pad) / (Self.size + Self.gap)).rounded(.down))
        guard y >= Self.pad, k >= 0, k < terminalApps.count,
              y - Self.pad - CGFloat(k) * (Self.size + Self.gap) <= Self.size else { return nil }
        return k
    }

    var body: some View {
        let edge: Edge = hub.boardOnLeft ? .leading : .trailing
        ZStack(alignment: hub.boardOnLeft ? .bottomLeading : .bottomTrailing) {
            if open {
                panel.transition(.move(edge: edge).combined(with: .opacity))
            } else {
                tab.transition(.move(edge: edge).combined(with: .opacity))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: hub.boardOnLeft ? .bottomLeading : .bottomTrailing)
        .padding(.bottom, Self.bottomInset)
        .animation(.spring(response: 0.32, dampingFraction: 0.86), value: open)
        .animation(.easeOut(duration: 0.2), value: pointer.inside)
        .onChange(of: pointer.atEdge) { sync() }
    }

    // 숨어 있을 때 창 오른쪽 아래 끝에 붙은 탭 — 여기에 마우스를 대면 터미널 독이 열린다는 표시.
    // 보드에 마우스가 들어오면 밝아지며 끝까지 튀어나온다. 열고 닫기는 창 전체 추적(PointerTracker)이 맡으므로 누르지는 않는다
    private var tab: some View {
        let left = hub.boardOnLeft
        let shape = left ? UnevenRoundedRectangle(bottomTrailingRadius: 8, topTrailingRadius: 8)
                         : UnevenRoundedRectangle(topLeadingRadius: 8, bottomLeadingRadius: 8)
        return VStack(spacing: 6) {
            Image(systemName: left ? "chevron.right" : "chevron.left").font(.system(size: 8, weight: .heavy))
            Image(systemName: "terminal.fill").font(.system(size: 11, weight: .semibold))
        }
        .foregroundStyle(Color.white.opacity(pointer.inside ? 0.95 : 0.6))
        .frame(width: Self.trigger, height: 58)
        .background(shape.fill(Palette.accent.opacity(pointer.inside ? 0.5 : 0.25)))
        .overlay(shape.stroke(Color.white.opacity(0.18), lineWidth: 0.5))
        .offset(x: pointer.inside ? 0 : left ? -4 : 4)
        .allowsHitTesting(false)
    }

    private var panel: some View {
        let left = hub.boardOnLeft
        // hover 는 아이콘마다가 아니라 독 전체에서 받는다 — 아이콘이 커지며 추적 영역이 바뀌면 들어옴·나감이 2ms 마다 번갈아 와 보드가 멈춘다
        return VStack(spacing: Self.gap) {
            ForEach(Array(terminalApps.enumerated()), id: \.element.id) { i, app in
                LauncherButton(app: app, icon: store.icons[app.id], running: store.running.contains(app.id), hovered: hovered == i, left: left)
            }
        }
        .padding(Self.pad)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(Color(red: 0.11, green: 0.11, blue: 0.17).opacity(0.82))
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
        )
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(Color.white.opacity(0.14), lineWidth: 0.5))
        .shadow(color: .black.opacity(0.45), radius: 14, x: left ? 3 : -3, y: 4)
        .contentShape(Rectangle())
        .onContinuousHover { phase in
            var i: Int?
            if case .active(let p) = phase { i = index(at: p.y) }
            if hovered != i { hovered = i }
            let over = phase != .ended
            if over != overPanel { overPanel = over; sync() }
        }
        .onTapGesture { location in
            if let i = index(at: location.y) { TerminalAppStore.shared.open(terminalApps[i].id) }
        }
        // 힌트는 독 안쪽 옆(보드 쪽)에 오버레이로 띄운다 — 아이콘을 옮겨 다니면 사라졌다 나타나지 않고 그 아이콘 높이로 미끄러진다
        .overlay(alignment: left ? .topLeading : .topTrailing) { hint.allowsHitTesting(false) }
        .animation(.spring(response: 0.26, dampingFraction: 0.86), value: hovered)
        .padding(left ? .leading : .trailing, 8)
    }

    @ViewBuilder private var hint: some View {
        if let i = hovered {
            let app = terminalApps[i]
            let installed = store.urls[app.id] != nil
            let running = store.running.contains(app.id)
            HStack(spacing: 5) {
                Image(systemName: installed ? (running ? "arrow.up.forward.app" : "plus.app") : "nosign")
                    .font(.system(size: 11, weight: .semibold))
                Text(installed ? (running ? "\(app.name) 창으로 이동" : "\(app.name) 실행")
                               : "\(app.name) · 설치되지 않음")
                    .boardFont(12, weight: .semibold)
                    .lineLimit(1)
                    .contentTransition(.opacity)
            }
            .fixedSize()
            .foregroundStyle(Color.white.opacity(installed ? 0.95 : 0.55))
            .padding(.horizontal, 10)
            .frame(height: Self.hintHeight)
            .background(Capsule().fill(Color(red: 0.10, green: 0.10, blue: 0.15).opacity(0.96)))
            .overlay(Capsule().stroke(Color.white.opacity(0.14), lineWidth: 0.5))
            .shadow(color: .black.opacity(0.4), radius: 8, y: 2)
            // 힌트 가운데를 그 아이콘 가운데 높이에, 오른쪽 끝을 독 왼쪽 8pt 앞에 둔다(보드가 왼쪽이면 왼쪽 끝을 독 오른쪽 8pt 뒤에)
            .offset(x: hub.boardOnLeft ? Self.width + 8 : -(Self.width + 8),
                    y: Self.pad + CGFloat(i) * (Self.size + Self.gap) + (Self.size - Self.hintHeight) / 2)
            .transition(.opacity.combined(with: .offset(x: hub.boardOnLeft ? -6 : 6)))
        }
    }
}

// 창 안 마우스 위치 — 투명 모드(마우스가 올라오면 불투명)와 오른쪽 독(오른쪽 끝에 대면 열림)이 함께 본다.
// SwiftUI hover 는 단추·줄마다 따로 잡혀 경계를 지날 때 끊기므로, 창 전체에 AppKit 추적 영역 하나를 둔다
final class PointerTracker: NSResponder, ObservableObject {
    static let shared = PointerTracker()
    @Published private(set) var inside = false
    @Published private(set) var atEdge = false
    private(set) var inWindow = false   // 창 어디든(펼친 창의 터미널 쪽·제목 막대 띠 포함) — 투명도가 쓴다
    private weak var view: NSView?
    private var windowArea: NSTrackingArea?

    private override init() { super.init() }
    required init?(coder: NSCoder) { fatalError() }

    func attach(_ v: NSView) {
        view = v
        v.addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .activeAlways, .inVisibleRect],
                                         owner: self))
    }

    // 창 전체 추적 — 보드 쪽 추적과 따로 둔다(독은 보드 쪽에 들어와야 튀어나온다)
    func attachWindow(_ v: NSView) {
        let a = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self)
        windowArea = a
        v.addTrackingArea(a)
    }

    override func mouseEntered(with event: NSEvent) {
        if event.trackingArea === windowArea { return setInWindow(true) }
        mouseMoved(with: event)
    }

    override func mouseExited(with event: NSEvent) {
        if event.trackingArea === windowArea { return setInWindow(false) }
        if atEdge { atEdge = false }
        setInside(false)
        HintPanel.shared.leaveAll()
    }

    override func mouseMoved(with event: NSEvent) {
        guard let v = view else { return }
        setInside(true)
        let p = v.convert(event.locationInWindow, from: nil)   // 왼쪽 아래가 원점
        // 독은 보드의 화면 바깥쪽 끝에 있다 — 보드 위치가 왼쪽이면 왼쪽 끝
        let outer = MainActor.assumeIsolated { TerminalHub.shared.boardOnLeft } ? p.x <= v.bounds.minX + EdgeDock.trigger
                                                                                  : p.x >= v.bounds.maxX - EdgeDock.trigger
        let edge = outer && p.y - v.bounds.minY <= EdgeDock.triggerHeight
        if edge != atEdge { atEdge = edge }
    }

    private func setInside(_ v: Bool) {
        guard v != inside else { return }
        inside = v
    }

    private func setInWindow(_ v: Bool) {
        guard v != inWindow else { return }
        inWindow = v
        WindowOpacity.shared.apply()
    }
}

// 투명 모드 — 카카오톡 PC 처럼 오른쪽 아래 슬라이더로 창 전체(글자 포함)의 투명도를 정한다.
// 마우스가 창 안에 있으면 불투명하게 보이고, 슬라이더를 움직이는 동안은 고른 투명도를 미리 보여 준다.
// 보드가 활성 창이어도 마우스가 창 밖이면 고른 투명도다(사용자 결정)
@MainActor final class WindowOpacity: ObservableObject {
    static let shared = WindowOpacity()
    static let range: ClosedRange<Double> = 0.3...1
    @Published var level = min(max(UserDefaults.standard.object(forKey: "opacity") as? Double ?? 1, WindowOpacity.range.lowerBound),
                               WindowOpacity.range.upperBound) {
        didSet { UserDefaults.standard.set(level, forKey: "opacity"); apply() }
    }
    @Published var previewing = false { didSet { apply() } }

    func apply() {
        guard let w = window else { return }
        // 터미널을 펼쳐 화면 전체를 덮은 동안에는 늘 불투명 — 화면 전체의 터미널이 비쳐 보이게 할 까닭이 없다
        let solid = !previewing && (PointerTracker.shared.inWindow || expandedToFullScreen() || SessionMenuPanel.shared.isOpen)
        let target = solid ? 1 : CGFloat(level)
        guard abs(w.alphaValue - target) > 0.001 else { return }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = previewing ? 0 : 0.22
            w.animator().alphaValue = target
        }
    }
}

// 오른쪽 아래 투명도 슬라이더 — 왼쪽으로 갈수록 투명하다
struct OpacitySlider: View {
    var compact = false     // 미니 화면에서는 막대를 짧게
    @ObservedObject private var opacity = WindowOpacity.shared
    @State private var hover = false
    @State private var settle: DispatchWorkItem?

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "circle.lefthalf.filled").font(.system(size: 10, weight: .semibold))
            Slider(value: $opacity.level, in: WindowOpacity.range) { editing in
                settle?.cancel()
                if editing { opacity.previewing = true; return }
                // 놓은 뒤 잠깐 고른 투명도를 더 보여 주고 불투명으로 돌아간다(마우스가 보드 위에 있으므로)
                let w = DispatchWorkItem { opacity.previewing = false }
                settle = w
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: w)
            }
            .controlSize(.mini)
            .tint(Palette.accent)
            .frame(width: compact ? 48 : 84)
            Text("\(Int((opacity.level * 100).rounded()))%")
                .boardFont(10, weight: .semibold).monospacedDigit()
                .fixedSize().frame(minWidth: 32, alignment: .trailing)   // 보드 글자를 키우면 "100%" 가 32 에 안 들어가 두 줄이 됐다
        }
        .foregroundStyle(.secondary)
        .opacity(hover ? 1 : 0.6)
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
        .hint("투명도: 마우스가 보드 창 밖에 있으면 보드를 쓰는 중이어도 이 투명도로 보입니다. 마우스를 올리면 불투명해집니다. 화면 전체로 펼친 창은 늘 불투명합니다")
    }
}

// 손가락 커서 — 보드는 초당 20번 다시 그려서 NSCursor.set() 이 곧 화살표로 되돌아간다. macOS 15 부터는 SwiftUI 포인터 모양을 쓴다
struct HandPointer: ViewModifier {
    let on: Bool
    func body(content: Content) -> some View {
        if #available(macOS 15.0, *) { content.pointerStyle(on ? .link : nil) } else { content }
    }
}

struct LauncherButton: View {
    let app: TerminalApp
    let icon: NSImage?
    let running: Bool
    let hovered: Bool
    var left = false        // 보드 위치가 왼쪽이면 독도 왼쪽 끝이라 바깥쪽이 왼쪽
    var body: some View {
        let installed = icon != nil
        ZStack {
            if let icon {
                Image(nsImage: icon).resizable().interpolation(.high)
            } else {
                RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.08))
                    .overlay(Image(systemName: "terminal").font(.system(size: 15, weight: .semibold)).foregroundStyle(.secondary))
                    .padding(3)
            }
        }
        .frame(width: EdgeDock.size, height: EdgeDock.size)
        .modifier(HandPointer(on: installed))
        .saturation(installed ? 1 : 0)
        .opacity(installed ? 1 : 0.35)
        .scaleEffect(installed && hovered ? 1.12 : 1)
        .animation(.spring(response: 0.25, dampingFraction: 0.6), value: hovered)
        // 실행 중이면 Dock 처럼 바깥쪽(오른쪽, 보드가 왼쪽이면 왼쪽)에 점
        .overlay(alignment: left ? .leading : .trailing) {
            Circle().fill(Color.white.opacity(running ? 0.8 : 0)).frame(width: 4, height: 4)
                .offset(x: left ? -(EdgeDock.pad / 2 + 2) : EdgeDock.pad / 2 + 2)
        }
    }
}

struct ScrollRequest: Equatable { let id: String; let n: Int }

// 목록 카드마다 스크롤 좌표의 윗변 위치
struct CardTops: PreferenceKey {
    static var defaultValue: [String: CGFloat] = [:]
    static func reduce(value: inout [String: CGFloat], nextValue: () -> [String: CGFloat]) {
        value.merge(nextValue()) { $1 }
    }
}

struct BoardView: View {
    @ObservedObject var store: Store
    @ObservedObject var usage: UsageStore
    @ObservedObject var accounts: AccountStore
    @AppStorage("pinned") private var pinned = true
    @AppStorage("usageCompact") private var usageCompact = false
    @State private var foldHover = false
    @State private var checkHover = false
    @State private var pinHover = false
    // 요약 칩: 걸러 보는 상태·스크롤 요청
    @State private var filter: String?
    @State private var scrollRequest = ScrollRequest(id: "", n: 0)
    // 목록 아래로 가려진 카드 — 카드 위치는 스크롤 좌표로 모은다
    // 스크롤·애니메이션 프레임마다 위치가 바뀌어도 화면은 "가려진 카드 목록" 이 달라질 때만 다시 그린다
    @State private var tops = CardTopsBox()
    @State private var hiddenIds: [String] = []
    @State private var moreHover = false
    // 창 크기 단계 — 중간에서는 카드를 접힌 한 줄로 보이고, 그 단계에서 눌러 펼친 카드만 따로 기억한다(저장한 접힘 상태는 그대로)
    @ObservedObject private var fit = BoardFit.shared
    @State private var mediumOpen: Set<String> = []
    @ObservedObject private var updater = Updater.shared
    // 자율주행 — 실행 줄과 자율주행 중·지난 실행 화면(보드 쪽 전체를 바꿈). 보기: 모두·기본(실행 줄 숨김)·자율주행(실행 줄만)
    @ObservedObject private var autopilot = AutopilotStore.shared
    @AppStorage("autopilotView") private var autopilotView = "all"

    private func updateHidden() {
        guard tops.viewport > 0 else { return }
        let ids = shownGroups.map(\.id).filter { (tops.values[$0] ?? 0) > tops.viewport - 24 }
        if ids != hiddenIds { hiddenIds = ids }
    }

    // 목록 칸·보드 높이는 한 번의 배치에서 함께 바뀌므로 둘 다 갱신된 뒤에 단계 판정에 넘긴다.
    // 그때 그려진 단계(tier)를 함께 넘겨, 단계가 바뀌는 사이에 앞 단계의 높이가 새 단계 값으로 잡히지 않게 한다
    private func measure(_ tier: BoardTier) {
        DispatchQueue.main.async { BoardFit.shared.measured(tier, board: tops.board, list: tops.viewport) }
    }

    // 목록 칸의 높이 — 가려진 카드 계산과 단계 판정이 함께 쓴다
    private var listHeightReader: some View {
        let tier = fit.tier
        return GeometryReader { geo in
            Color.clear.onAppear { tops.viewport = geo.size.height; updateHidden(); measure(tier) }
                .onChange(of: geo.size.height) { _, h in tops.viewport = h; updateHidden(); measure(tier) }
        }
    }

    // 칩 한 번 = 그 상태만 보기(해당 카드를 펼치고 맨 위로), 같은 칩 다시 = 모두 보기
    private func chipTapped(_ st: String) {
        if filter == st { withAnimation(.easeOut(duration: 0.18)) { filter = nil }; return }
        let ids = store.groups.filter { $0.sessions.contains { $0.state == st } }.map(\.id)
        withAnimation(.easeOut(duration: 0.18)) {
            filter = st
            ids.forEach(store.reveal)
        }
        if let first = ids.first { scrollRequest = ScrollRequest(id: first, n: scrollRequest.n + 1) }
    }

    // 걸러 보는 중이면 그 상태 세션만 남긴 카드
    private var shownGroups: [Group] {
        guard let f = filter else { return store.groups }
        return store.groups.compactMap { g in
            let ss = g.sessions.filter { $0.state == f }
            return ss.isEmpty ? nil : Group(id: g.id, sessions: ss)
        }
    }

    // 화면 아래로 가려진 카드 수와 그중 승인 대기가 있는지
    private var below: (n: Int, waiting: Bool, first: String?) {
        let ids = Set(hiddenIds)
        let hidden = shownGroups.filter { ids.contains($0.id) }
        return (hidden.count, hidden.contains { $0.sessions.contains { $0.state == "waiting" } }, hidden.first?.id)
    }

    private var titleBlock: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Image(nsImage: appIcon).resizable().interpolation(.high).frame(width: 24, height: 24)
                Text("Claude Board").boardFont(19, weight: .bold).lineLimit(1)
                if !updater.current.isEmpty {
                    // build-board.sh 로 만든 개발용 빌드는 버전이 1.0 이다
                    Text(updater.current == "1.0" ? "dev" : updater.current)
                        .boardFont(12, weight: .semibold, design: .monospaced)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .background(Capsule().fill(Color.white.opacity(0.08)))
                        .lineLimit(1).fixedSize()
                        .hint("Claude Board 버전")
                }
            }
            let active = store.groups.reduce(0) { $0 + $1.sessions.filter { $0.state != "ended" }.count }
            // Claude Code 버전은 부가 정보라 세션 수 옆에 흐리게 붙인다
            HStack(spacing: 0) {
                Text("활성 세션 \(active)개").contentTransition(.numericText())
                if let v = claudeVersion() {
                    Text(" · Claude Code \(v)").foregroundStyle(.tertiary)
                        .hint("설치된 Claude Code 버전")
                }
            }
            .boardFont(12).foregroundStyle(.secondary).lineLimit(1)
        }
    }

    private var headerControls: some View {
        HStack {
            HStack(spacing: 6) {
                ForEach(["waiting", "working", "idle"], id: \.self) { st in
                    let n = store.count(st)
                    if n > 0 {
                        SummaryChip(state: st, n: n, filtering: filter == st) { chipTapped(st) }
                            .transition(.scale.combined(with: .opacity))
                    }
                }
            }
            Spacer(minLength: 6)
            AllMuteButton(muted: store.allMuted) { store.setAllMuted(!store.allMuted) }
            // 업데이트 확인 — 새 버전이 있으면 아래 알림 줄에 업데이트 단추가, 없으면 "최신 버전입니다" 가 잠깐 뜬다
            Button { updater.check(manual: true) } label: {
                Image(systemName: "arrow.triangle.2.circlepath")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(checkHover ? .primary : .secondary)
                    .opacity(updater.checking ? 0.4 : 1)
                    .padding(.horizontal, 8)
                    .frame(minWidth: 28, minHeight: 24)
                    .background(Capsule().fill(Color.white.opacity(checkHover ? 0.12 : 0.07)))
            }
            .buttonStyle(.plain)
            .clickable($checkHover)
            .disabled(updater.checking)
            .hint("업데이트 확인: GitHub 에 새 버전이 있는지 확인합니다")
            // 모두 접기 = 경로 카드를 다 접고 사용 한도를 간략(한 줄)으로, 모두 펼치기 = 다 펴고 상세로.
            // 중간 화면은 이미 카드가 접힌 한 줄·사용 한도가 한 줄이라 숨긴다
            let all = usageCompact && store.groups.allSatisfy { store.collapsed.contains($0.id) }
            if fit.tier == .full {
            Button {
                withAnimation(foldSpring) {
                    store.setAll(collapse: !all)
                    usageCompact = !all
                }
            } label: {
                // 마우스를 올려도 폭이 변하지 않게 글자는 도움말로만 보인다 — 폭이 바뀌면 "위에 고정" 이 줄었다 늘었다 한다
                Image(systemName: all ? "rectangle.expand.vertical" : "rectangle.compress.vertical")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(foldHover ? .primary : .secondary)
                    .padding(.horizontal, 8)
                    .frame(minWidth: 28, minHeight: 24)
                    .background(Capsule().fill(Color.white.opacity(foldHover ? 0.12 : 0.07)))
            }
            .buttonStyle(.plain)
            .clickable($foldHover)
            .hint(all ? "모두 펼치기: 경로 카드를 펴고 사용 한도를 자세히" : "모두 접기: 경로 카드를 접고 사용 한도를 한 줄로")
            }
            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.6)) { setPinned(!pinned) }
            } label: {
                // 윗줄이 좁으면 글자를 빼고 압정 아이콘만 보인다
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 4) {
                        Image(systemName: pinned ? "pin.fill" : "pin.slash")
                            .rotationEffect(.degrees(pinned ? 0 : 45))
                        Text(pinned ? "위에 고정" : "고정 안 함").fixedSize()
                    }
                    Image(systemName: pinned ? "pin.fill" : "pin.slash")
                        .rotationEffect(.degrees(pinned ? 0 : 45))
                }
                .boardFont(11, weight: .semibold)
                .foregroundStyle(pinned ? Palette.accent : .secondary)
                .padding(.horizontal, 9)
                .frame(height: 24)
                .background(Capsule().fill(pinned ? Palette.accent.opacity(0.16) : Color.white.opacity(0.07)))
                .overlay(Capsule().stroke(pinned ? Palette.accent.opacity(0.4) : .clear, lineWidth: 0.5))
            }
            .buttonStyle(.plain)
            .clickable($pinHover)
            .hint(pinned ? "항상 위에 표시 끄기" : "항상 위에 표시")
        }
    }

    // 창 크기 단계에 따라 미니 화면 또는 전체·중간 화면
    var body: some View {
        ZStack(alignment: .bottom) {
            switch autopilot.screen {
            case .running(let id):
                APRunningScreen(ap: autopilot, id: id)
            case .history(let selected):
                APHistoryScreen(ap: autopilot, selected: selected)
            case .board:
                if fit.tier == .mini {
                    MiniBoard(store: store, usage: usage)
                        .background(Backdrop())
                        .preferredColorScheme(.dark)
                } else {
                    board
                }
            }
            TerminalNotice(hub: TerminalHub.shared)
        }
        .boardFont(13)   // 글자 크기를 따로 정하지 않은 글자(단추 제목 등)도 보드 글자 비율을 따른다
    }

    private var board: some View {
        let tier = fit.tier
        let medium = tier == .medium
        let runs = autopilot.runs
        let autoOnly = autopilotView == "auto" && !runs.isEmpty   // 도는 실행이 없으면 모두 보기로 그린다
        // 경로 카드가 없는 실행 — 자율주행 세션은 세션 파일을 남기지 않아 사람 세션이 없던 경로에는 카드가 없다
        let orphans = autopilotView == "basic" || filter != nil ? [] : runs.filter { r in !store.groups.contains { r.covers($0.id) } }
        return VStack(alignment: .leading, spacing: 14) {
            // 윗줄은 늘 두 줄 — 제목 아래 줄에 상태 칩(왼쪽)과 단추(오른쪽).
            // 한 줄·두 줄을 폭에 따라 바꾸면 "알림 꺼짐" 글자가 생길 때마다 단추 줄이 위아래로 오간다
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .center, spacing: 8) {
                    titleBlock.layoutPriority(1)
                    Spacer(minLength: 0)
                    Foreman(groups: store.groups, limits: usage.limits)
                }
                headerControls
                APBar(ap: autopilot, view: $autopilotView)
            }
            .padding(.top, 6)

            if updater.latest != nil || updater.notice != nil {
                UpdateBanner(updater: updater).transition(.move(edge: .top).combined(with: .opacity))
            }

            UsagePanel(usage: usage, accounts: accounts, forceCompact: medium,
                       stoplines: autoOnly ? autopilot.stoplines : nil) {
                store.groups.flatMap(\.sessions).filter { $0.state == "working" || $0.state == "waiting" }
            }

            if store.groups.isEmpty && runs.isEmpty && autopilot.awaiting.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "moon.zzz.fill").font(.system(size: 30)).foregroundStyle(.secondary)
                    Text("아직 기록된 세션이 없습니다").boardFont(14, weight: .medium)
                    Text("Claude Code 세션에서 요청을 보내면 여기에 나타납니다")
                        .boardFont(12).foregroundStyle(.secondary)
                }
                // 창이 낮으면 안내가 줄어들며 잘린다 — 줄지 않으면 보드가 넘쳐 아래 투명도 슬라이더가 밀려난다
                .frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
                .clipped()
                .background(listHeightReader)
                .transition(.opacity)
            } else {
                if let f = filter {
                    HStack(spacing: 6) {
                        Circle().fill(Palette.color(f)).frame(width: 7, height: 7)
                        Text("\(Palette.name(f))만 보는 중").boardFont(12, weight: .semibold)
                        Spacer()
                        Button("모두 보기") { withAnimation(foldSpring) { filter = nil } }
                            .buttonStyle(.plain).boardFont(12, weight: .semibold).foregroundStyle(Palette.accent)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 6)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Palette.color(f).opacity(0.12)))
                    .transition(.move(edge: .top).combined(with: .opacity))
                }
                ScrollViewReader { proxy in
                ScrollView(showsIndicators: true) {
                    VStack(spacing: 12) {
                        // 끝났고 합치기·승격을 기다리는 실행 — 기본 보기(실행 줄 숨김)에서는 빼되, 도는 실행이 없어 보기 단추가 없을 때는 보인다
                        if autopilotView != "basic" || runs.isEmpty {
                            ForEach(autopilot.awaiting) { r in APEndedLine(run: r).equatable().transition(.opacity) }
                        }
                        // 자율주행 보기는 실행 카드만, 다른 보기는 카드 없는 실행을 맨 위에. 단위 시계(분)는 1초 시계(EverySecond)로 셈한다
                        ForEach(autoOnly ? runs : orphans) { r in
                            EverySecond { now in APRunCard(run: r, minutes: r.unitMinutes(now), meters: !autoOnly).equatable() }
                                .transition(.opacity)
                        }
                        ForEach(autoOnly ? [] : shownGroups) { g in
                            GroupCard(g: g,
                                      collapsed: medium ? !mediumOpen.contains(g.id) : store.collapsed.contains(g.id),
                                      toggle: { if medium { mediumOpen.formSymmetricDifference([g.id]) } else { store.toggle(g.id) } },
                                      endedOpen: store.endedOpen.contains(g.id), toggleEnded: { store.toggleEnded(g.id) },
                                      pinned: store.pinnedGroups.contains(g.id), togglePin: { store.togglePinGroup(g.id) },
                                      remove: { withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.hideGroup(g) } },
                                      clearEnded: { withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.removeEnded(g) } })
                                .transition(.asymmetric(insertion: .move(edge: .top).combined(with: .opacity),
                                                        removal: .opacity))
                                .id(g.id)
                                .background(GeometryReader { geo in
                                    Color.clear.preference(key: CardTops.self,
                                                           value: [g.id: geo.frame(in: .named("boardScroll")).minY])
                                })
                        }
                    }
                    .padding(.bottom, 12)
                }
                .coordinateSpace(name: "boardScroll")
                .background(listHeightReader)
                .onPreferenceChange(CardTops.self) { tops.values = $0; updateHidden() }
                .onChange(of: scrollRequest) { _, r in
                    proxy.scrollTo(r.id, anchor: .top)   // 걸러 보기 전환과 겹치지 않게 애니메이션 없이
                }
                .overlay(alignment: .bottom) {
                    let b = below
                    if b.n > 0 {
                        ZStack(alignment: .bottom) {
                            // 아래 끝을 흐리게 — 목록이 더 있다는 표시
                            LinearGradient(colors: [.clear, Color(red: 0.07, green: 0.07, blue: 0.11).opacity(0.9)],
                                           startPoint: .top, endPoint: .bottom)
                                .frame(height: 56)
                                .allowsHitTesting(false)
                            let tint = b.waiting ? Palette.color("waiting") : Color.white
                            HStack(spacing: 5) {
                                Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold))
                                Text(b.waiting ? "아래에 경로 \(b.n)개 더 · 승인 대기 있음" : "아래에 경로 \(b.n)개 더")
                                    .boardFont(11, weight: .semibold)
                            }
                            .foregroundStyle(b.waiting ? Color.black.opacity(0.85) : Color.white.opacity(0.9))
                            .padding(.horizontal, 10).padding(.vertical, 5)
                            .background(Capsule().fill(b.waiting ? tint.opacity(moreHover ? 1 : 0.9) : tint.opacity(moreHover ? 0.22 : 0.14)))
                            .overlay(Capsule().stroke(tint.opacity(0.35), lineWidth: 0.5))
                            .contentShape(Capsule())
                            .onTapGesture {
                                if let id = b.first {
                                    withAnimation(.spring(response: 0.5, dampingFraction: 0.86)) { proxy.scrollTo(id, anchor: .top) }
                                }
                            }
                            .clickable($moreHover)
                            .hint("눌러서 아래 카드로 이동")
                            .padding(.bottom, 8)
                        }
                        .transition(.opacity)
                    }
                }
                }
                .environment(\.notifyLocked, store.allMuted)
            }

            // 카카오톡 PC 처럼 창 오른쪽 아래에 투명도 슬라이더
            HStack {
                Spacer()
                OpacitySlider()
            }
            .padding(.top, -8)
            .padding(.bottom, 6)
        }
        .padding(.horizontal, 16)
        .padding(.top, 22)
        // 최소 높이는 주지 않는다 — 주면 제목 막대 높이만큼 넘쳐 아래 투명도 슬라이더가 잘린다. 낮으면 단계가 바뀐다
        .frame(minWidth: boardMinWidth)
        .background(GeometryReader { geo in
            Color.clear.onAppear { tops.board = geo.size.height; measure(tier) }
                .onChange(of: geo.size.height) { _, h in tops.board = h; measure(tier) }
        })
        .background(Backdrop())
        // 터미널 열기 독 — 보드 위에 겹쳐 뜨므로 아래 배치는 그대로다. 붙는 쪽(보드 위치)은 독이 스스로 정한다
        .overlay { EdgeDock() }
        .onChange(of: store.groups) { _, _ in
            if let f = filter, store.count(f) == 0 { withAnimation(foldSpring) { filter = nil } }
        }
        .preferredColorScheme(.dark)
        .animation(.spring(response: 0.45, dampingFraction: 0.82), value: store.groups)
    }
}
