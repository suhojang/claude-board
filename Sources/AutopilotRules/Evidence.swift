// 자율주행 규칙(AutopilotRules) — 근거 묶음과 MANIFEST.json(4단계 ⑩-2, 2026-10-30, 설계 5장 6절 "근거 묶음 파일(읽기 전용, MANIFEST.json 에
// 출처·읽은 시각·해시·가린 비밀 수)"). 읽은 것 하나가 메타 파일 하나(<n>-<출처>-<id>.json)와 Figma 면 그림 한 장(<n>-figma-<id>.png)이고,
// MANIFEST 가 그 목록이다. 같은 주소를 다시 읽으면 같은 번호의 파일을 바꾼다. 파일 쓰기(임시 + rename, 0444, 링크 따라가지 않음)와 해시는 어댑터 몫
import Foundation

public let apEvidenceFormat = "claude-board-evidence"
public let apManifestName = "MANIFEST.json"

public struct APEvidenceImage: Equatable, Sendable {
    public var file = ""
    public var host = ""
    public var sha256 = ""
    public var bytes = 0
    public init(file: String = "", host: String = "", sha256: String = "", bytes: Int = 0) { self.file = file; self.host = host; self.sha256 = sha256; self.bytes = bytes }
}

public struct APEvidenceEntry: Equatable, Sendable {
    public var n = 0
    public var file = ""
    public var source = ""        // jira·confluence·notion·figma·mcp:<서버>
    public var via = ""           // rest·mcp
    public var url = ""
    public var id = ""
    public var title = ""
    public var readAt = 0.0
    public var sha256 = ""
    public var bytes = 0
    public var redacted = 0
    public var requests = 0
    public var costUSD: Double?
    public var image: APEvidenceImage?
    public init() {}
}

// 파일 이름 — 번호·출처·id(쓸 수 있는 글자만, 40자까지)
public func apEvidenceName(_ n: Int, source: String, id: String) -> String {
    func safe(_ x: String, _ k: Int) -> String {
        let t = String(x.map { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "_" || $0 == "-") ? $0 : "-" })
            .replacingOccurrences(of: "-{2,}", with: "-", options: .regularExpression).trimmingCharacters(in: CharacterSet(charactersIn: "-"))
        return String(t.prefix(k))
    }
    let src = safe(source.replacingOccurrences(of: ":", with: "-"), 40)
    let i = safe(id, 40)
    return "\(n)-\(src.isEmpty ? "web" : src)" + (i.isEmpty ? "" : "-" + i) + ".json"
}

public func apManifestJSON(_ entries: [APEvidenceEntry], updated: Double) -> [String: Any] {
    ["format": apEvidenceFormat, "version": 1, "updated": updated, "redacted": entries.reduce(0) { $0 + $1.redacted },
     "entries": entries.sorted { $0.n < $1.n }.map { e -> [String: Any] in
        var d: [String: Any] = ["n": e.n, "file": e.file, "source": e.source, "via": e.via, "url": e.url, "id": e.id, "title": e.title,
                                "read_at": e.readAt, "sha256": e.sha256, "bytes": e.bytes, "redacted": e.redacted, "requests": e.requests]
        if let c = e.costUSD { d["cost_usd"] = c }
        if let i = e.image { d["image"] = ["file": i.file, "host": i.host, "sha256": i.sha256, "bytes": i.bytes] }
        return d
     }]
}
public func apManifestData(_ entries: [APEvidenceEntry], updated: Double) -> Data {
    (try? JSONSerialization.data(withJSONObject: apManifestJSON(entries, updated: updated), options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data()
}

// MANIFEST 읽기 — 꼴이 틀린 줄은 버린다(파일 이름이 묶음 밖을 가리키지 않게 이름 꼴도 본다)
public func apManifestParse(_ data: Data?) -> [APEvidenceEntry] {
    guard let data, data.count <= 4 << 20, let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
          o["format"] as? String == apEvidenceFormat, let list = o["entries"] as? [[String: Any]] else { return [] }
    func name(_ v: Any?, _ ext: String) -> String? {
        guard let s = v as? String, s.range(of: "^[0-9]{1,4}-[A-Za-z0-9_-]{1,90}\\.\(ext)$", options: .regularExpression) != nil else { return nil }
        return s
    }
    var out: [APEvidenceEntry] = [], seen = Set<Int>()
    for d in list.prefix(500) {
        guard let n = (d["n"] as? NSNumber)?.intValue, n >= 1, seen.insert(n).inserted, let f = name(d["file"], "json") else { continue }
        var e = APEvidenceEntry()
        e.n = n; e.file = f
        e.source = d["source"] as? String ?? ""; e.via = d["via"] as? String ?? ""; e.url = d["url"] as? String ?? ""
        e.id = d["id"] as? String ?? ""; e.title = d["title"] as? String ?? ""
        e.readAt = (d["read_at"] as? NSNumber)?.doubleValue ?? 0; e.sha256 = d["sha256"] as? String ?? ""
        e.bytes = (d["bytes"] as? NSNumber)?.intValue ?? 0; e.redacted = (d["redacted"] as? NSNumber)?.intValue ?? 0
        e.requests = (d["requests"] as? NSNumber)?.intValue ?? 0; e.costUSD = (d["cost_usd"] as? NSNumber)?.doubleValue
        if let i = d["image"] as? [String: Any], let f = name(i["file"], "png") {
            e.image = APEvidenceImage(file: f, host: i["host"] as? String ?? "", sha256: i["sha256"] as? String ?? "", bytes: (i["bytes"] as? NSNumber)?.intValue ?? 0)
        }
        out.append(e)
    }
    return out.sorted { $0.n < $1.n }
}

public struct APEvidenceWrite: Equatable, Sendable {
    public var entry: APEvidenceEntry?
    public var note = ""
    public init(entry: APEvidenceEntry? = nil, note: String = "") { self.entry = entry; self.note = note }
}

// 읽은 것 하나를 묶음에 넣는다 — 같은 주소면 같은 번호(옛 파일은 지움), 아니면 다음 번호. 그림 파일 이름을 메타 image 칸에 적는다.
// 모든 길(내장 REST·MCP 읽기 세션·단위 시작 전 티켓)이 이 문을 지나므로 비밀 가리기(apMetaRedact)를 여기서 늘 하고, 가린 뒤 형식 검사(apMetaCheck)를
// 다시 지난 것만 쓴다. redacted 는 앞서 가린 수(내장 읽기가 먼저 가렸으면 그 수)이고 여기서 더 가린 수를 더해 MANIFEST 에 적는다
// (2026-10-30 자동 보안 검토 MEDIUM: 전에는 MCP 길의 가리기를 엔진이 따로 했다). 메타 파일 → 그림 → MANIFEST 차례(MANIFEST 가 가리키는 파일은 늘 있다)
public func apAddEvidence(_ meta0: APMeta, image: Data?, imageHost: String, url: String, via: String, requests: Int, redacted: Int,
                          cost: Double? = nil, store: APEvidenceStore, clock: APClock) -> APEvidenceWrite {
    var entries = apManifestParse(store.read(apManifestName))
    let old = entries.first { $0.url == url }
    let n = old?.n ?? ((entries.map(\.n).max() ?? 0) + 1)
    let (red, more) = apMetaRedact(meta0)
    let file = apEvidenceName(n, source: red.source, id: red.id)
    var meta = red
    let imageFile = image == nil ? "" : String(file.dropLast(5)) + ".png"
    meta.image = imageFile
    let checked = apMetaCheck(apMetaJSON(meta))
    guard let ok = checked.meta else { return APEvidenceWrite(note: "형식 검사에 걸려 넣지 않았습니다: " + checked.problems.prefix(3).joined(separator: " · ")) }
    let data = apMetaData(ok)
    guard let sha = store.write(file, data) else { return APEvidenceWrite(note: "근거 묶음에 \(file) 을 쓰지 못했습니다") }
    var e = APEvidenceEntry()
    e.n = n; e.file = file; e.source = ok.source; e.via = via; e.url = url; e.id = ok.id; e.title = ok.title
    e.readAt = clock.now.timeIntervalSince1970; e.sha256 = sha; e.bytes = data.count; e.redacted = redacted + more; e.requests = requests; e.costUSD = cost
    if let image {
        guard let ish = store.write(imageFile, image) else { store.remove(file); return APEvidenceWrite(note: "근거 묶음에 \(imageFile) 을 쓰지 못했습니다") }
        e.image = APEvidenceImage(file: imageFile, host: imageHost, sha256: ish, bytes: image.count)
    }
    if let old {
        if old.file != file { store.remove(old.file) }
        if let oi = old.image?.file, oi != imageFile { store.remove(oi) }
    }
    entries.removeAll { $0.n == n }
    entries.append(e)
    guard store.write(apManifestName, apManifestData(entries, updated: e.readAt)) != nil else { return APEvidenceWrite(entry: e, note: "MANIFEST.json 을 쓰지 못했습니다") }
    return APEvidenceWrite(entry: e, note: "근거 묶음에 넣었습니다")
}
