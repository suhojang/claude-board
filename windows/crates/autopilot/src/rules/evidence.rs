//! 근거 묶음과 `MANIFEST.json`(원본 `Sources/AutopilotRules/Evidence.swift`, 설계 5장 6절).
//! 읽은 것 하나가 메타 파일 하나(`<n>-<출처>-<id>.json`)와 Figma 면 그림 한 장(`<n>-figma-<id>.png`)이고, MANIFEST 가 그 목록이다.
//! 같은 주소를 다시 읽으면 같은 번호의 파일을 바꾼다. 파일 쓰기(임시 + rename, 읽기 전용)와 해시는 어댑터 몫이다.
//!
//! 이 파일의 이름은 모두 묶음 폴더 안의 맨 파일 이름(`&str`)이다. 경로 구분자(`/`·`\`)나 `..` 는 이름 검사가 거른다.
//! 폴더 경로 합치기·정규화는 부르는 쪽 몫이고, 여기서는 파일 시스템을 부르지 않는다.
//!
//! 파일 끝의 `add_evidence`(원본 `apAddEvidence`)는 그 재료를 엮는 문이다 — 메타 가리기(`meta_redact`)·형식 검사(`meta_check`)·
//! 저장소(`EvidenceStore`)·시계(`ReadClock`)를 묶어 메타 파일과 `MANIFEST.json` 을 쓴다.

use crate::rules::meta::{dim, meta_check, meta_data, meta_json, meta_redact, meta_wrap_untrusted, write_value, Meta};
use crate::rules::reading::ReadClock;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// 원본: `apEvidenceFormat`
pub const EVIDENCE_FORMAT: &str = "claude-board-evidence";
/// 원본: `apManifestName`
pub const MANIFEST_NAME: &str = "MANIFEST.json";

/// MANIFEST 를 읽을 때 받는 최대 바이트(4 MiB)
const MANIFEST_MAX_BYTES: usize = 4 << 20;
/// MANIFEST 를 읽을 때 받는 최대 항목 수
const MANIFEST_MAX_ENTRIES: usize = 500;

/// 원본: `APEvidenceImage`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EvidenceImage {
    pub file: String,
    pub host: String,
    pub sha256: String,
    pub bytes: i64,
}

/// 원본: `APEvidenceEntry`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EvidenceEntry {
    pub n: i64,
    pub file: String,
    /// jira·confluence·notion·figma·mcp:<서버>
    pub source: String,
    /// rest·mcp
    pub via: String,
    pub url: String,
    pub id: String,
    pub title: String,
    pub read_at: f64,
    pub sha256: String,
    pub bytes: i64,
    pub redacted: i64,
    pub requests: i64,
    pub cost_usd: Option<f64>,
    pub image: Option<EvidenceImage>,
}

/// 원본: `APEvidenceWrite`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EvidenceWrite {
    pub entry: Option<EvidenceEntry>,
    pub note: String,
}

/// 쓸 수 있는 글자(ASCII 영숫자·`_`·`-`)만 남기고 나머지는 `-`, 연속 `-` 는 하나로, 양끝 `-` 는 떼고, `k` 글자까지.
/// Swift 는 글자(자소 묶음) 단위라 결합 문자 묶음이 `-` 하나가 되지만 여기는 코드 포인트 단위다. 근거 이름은 ASCII 주소·id 에서 오므로 차이는 없다
fn safe_part(x: &str, k: usize) -> String {
    let mut out = String::new();
    for c in x.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').chars().take(k).collect()
}

/// 원본: `apEvidenceName`. 파일 이름 — 번호·출처·id(쓸 수 있는 글자만, 40자까지)
pub fn evidence_name(n: i64, source: &str, id: &str) -> String {
    let src = safe_part(&source.replace(':', "-"), 40);
    let i = safe_part(id, 40);
    let src = if src.is_empty() { "web" } else { &src };
    if i.is_empty() {
        format!("{n}-{src}.json")
    } else {
        format!("{n}-{src}-{i}.json")
    }
}

/// 키를 정렬해 넣은 객체(원본의 `.sortedKeys`). `preserve_order` 라 넣은 차례가 곧 출력 차례다
fn sorted_object(mut pairs: Vec<(&str, Value)>) -> Value {
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<Map<String, Value>>())
}

/// 원본: `apManifestJSON`. 항목은 번호 순이다. 어댑터가 쓰는 `MANIFEST.json` 의 내용.
pub fn manifest_json(entries: &[EvidenceEntry], updated: f64) -> Value {
    let mut sorted: Vec<&EvidenceEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.n);
    let list: Vec<Value> = sorted
        .into_iter()
        .map(|e| {
            let mut d = vec![
                ("n", Value::from(e.n)),
                ("file", Value::from(e.file.as_str())),
                ("source", Value::from(e.source.as_str())),
                ("via", Value::from(e.via.as_str())),
                ("url", Value::from(e.url.as_str())),
                ("id", Value::from(e.id.as_str())),
                ("title", Value::from(e.title.as_str())),
                ("read_at", dim(e.read_at)),
                ("sha256", Value::from(e.sha256.as_str())),
                ("bytes", Value::from(e.bytes)),
                ("redacted", Value::from(e.redacted)),
                ("requests", Value::from(e.requests)),
            ];
            if let Some(c) = e.cost_usd {
                d.push(("cost_usd", dim(c)));
            }
            if let Some(i) = &e.image {
                d.push((
                    "image",
                    sorted_object(vec![
                        ("file", Value::from(i.file.as_str())),
                        ("host", Value::from(i.host.as_str())),
                        ("sha256", Value::from(i.sha256.as_str())),
                        ("bytes", Value::from(i.bytes)),
                    ]),
                ));
            }
            sorted_object(d)
        })
        .collect();
    sorted_object(vec![
        ("format", Value::from(EVIDENCE_FORMAT)),
        ("version", Value::from(1)),
        ("updated", dim(updated)),
        ("redacted", Value::from(entries.iter().map(|e| e.redacted).sum::<i64>())),
        ("entries", Value::Array(list)),
    ])
}

/// 원본: `apManifestData`. 들여쓰기 2칸·키 정렬·`"키" : 값` 꼴·슬래시 이스케이프 없음 — 원본 JSONSerialization 쓰기를 그대로 쓴다(`meta_data` 가 쓰는 Swift JSON 쓰기).
/// ReadingTests `묶음` 의 골든이 MANIFEST 바이트를 비교하므로 꼴이 원본과 달라도 시험이 붉다.
pub fn manifest_data(entries: &[EvidenceEntry], updated: f64) -> Vec<u8> {
    let v = manifest_json(entries, updated);
    let mut out = String::new();
    if write_value(&mut out, &v, 0) {
        out.into_bytes()
    } else {
        Vec::new()
    }
}

/// 묶음 안 파일 이름 꼴 `^[0-9]{1,4}-[A-Za-z0-9_-]{1,90}\.<ext>$`(맨 이름만, 경로 구분자는 안 받는다).
/// Swift 의 `$` 는 끝 줄바꿈 앞에서도 맞지만 여기는 정확히 끝에서만 맞는다(더 엄격)
fn bundle_name(v: Option<&Value>, ext: &str) -> Option<String> {
    let s = v?.as_str()?;
    let stem = s.strip_suffix(ext)?.strip_suffix('.')?;
    let (num, rest) = stem.split_once('-')?;
    let num_ok = (1..=4).contains(&num.len()) && num.bytes().all(|b| b.is_ascii_digit());
    let rest_ok = (1..=90).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    (num_ok && rest_ok).then(|| s.to_string())
}

fn text_of(d: &Value, key: &str) -> String {
    d.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// 수 칸: 정수면 그대로, 실수면 소수를 버린다(Swift `NSNumber.intValue`). 수가 아니면 `None`
fn int_of(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// 원본: `apManifestParse`. MANIFEST 읽기 — 꼴이 틀린 줄은 버린다(파일 이름이 묶음 밖을 가리키지 않게 이름 꼴도 본다).
/// 4 MiB 를 넘거나 `format` 이 다르거나 `entries` 가 객체 배열이 아니면 빈 목록이다.
/// 항목은 500개까지, 번호는 1 이상이고 겹치면 앞의 것만 받는다(이름이 틀려 버린 항목도 그 번호를 쓴 것으로 센다).
pub fn manifest_parse(data: Option<&[u8]>) -> Vec<EvidenceEntry> {
    let Some(data) = data.filter(|d| d.len() <= MANIFEST_MAX_BYTES) else { return Vec::new() };
    let Ok(root) = serde_json::from_slice::<Value>(data) else { return Vec::new() };
    if root.get("format").and_then(Value::as_str) != Some(EVIDENCE_FORMAT) {
        return Vec::new();
    }
    let Some(list) = root.get("entries").and_then(Value::as_array) else { return Vec::new() };
    if !list.iter().all(Value::is_object) {
        return Vec::new();
    }
    let mut out: Vec<EvidenceEntry> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for d in list.iter().take(MANIFEST_MAX_ENTRIES) {
        let Some(n) = int_of(d.get("n")).filter(|n| *n >= 1) else { continue };
        if !seen.insert(n) {
            continue;
        }
        let Some(file) = bundle_name(d.get("file"), "json") else { continue };
        let image = d.get("image").filter(|i| i.is_object()).and_then(|i| {
            bundle_name(i.get("file"), "png").map(|file| EvidenceImage {
                file,
                host: text_of(i, "host"),
                sha256: text_of(i, "sha256"),
                bytes: int_of(i.get("bytes")).unwrap_or(0),
            })
        });
        out.push(EvidenceEntry {
            n,
            file,
            source: text_of(d, "source"),
            via: text_of(d, "via"),
            url: text_of(d, "url"),
            id: text_of(d, "id"),
            title: text_of(d, "title"),
            read_at: d.get("read_at").and_then(Value::as_f64).unwrap_or(0.0),
            sha256: text_of(d, "sha256"),
            bytes: int_of(d.get("bytes")).unwrap_or(0),
            redacted: int_of(d.get("redacted")).unwrap_or(0),
            requests: int_of(d.get("requests")).unwrap_or(0),
            cost_usd: d.get("cost_usd").and_then(Value::as_f64),
            image,
        });
    }
    out.sort_by_key(|e| e.n);
    out
}

// 원본 Evidence.swift 87~126줄.

/// 원본: `APEvidenceStore`. 근거 묶음 파일 저장소 — 어댑터가 읽기 전용(임시 파일 + rename)으로 구현한다.
/// 파일 이름은 언제나 맨 이름(`MANIFEST.json`·`<n>-<출처>-<id>.json` 꼴)이고 폴더 경로 합치기는 어댑터 몫이다.
pub trait EvidenceStore {
    /// 파일 읽기 — 없으면 `None`
    fn read(&self, name: &str) -> Option<Vec<u8>>;
    /// 파일 쓰기 — 성공하면 파일 해시(`sha256`), 실패하면 `None`. 같은 이름이 이미 있으면 바꾼다
    fn write(&self, name: &str, data: &[u8]) -> Option<String>;
    /// 파일 지우기 — 파일이 없어도 조용히 지나간다(원본과 같다)
    fn remove(&self, name: &str);
}

/// 원본: `apAddEvidence`. 읽은 것 하나를 근거 묶음에 넣는다 — 같은 주소면 같은 번호(옛 파일은 지움), 아니면 다음 번호. 그림 파일 이름을 메타 image 칸에 적는다.
/// 모든 길(내장 REST·MCP 읽기 세션·단위 시작 전 티켓)이 이 문을 지나므로 비밀 가리기(`meta_redact`)를 여기서 늘 하고, 가린 뒤 형식 검사(`meta_check`)를
/// 다시 지난 것만 쓴다(절 글은 믿지 않는 자료 표시로 감싸 쓴다, `meta_wrap_untrusted`). `redacted` 는 앞서 가린 수(내장 읽기가 먼저 가렸으면 그 수)이고
/// 여기서 더 가린 수를 더해 MANIFEST 에 적는다(2026-10-30 자동 보안 검토 MEDIUM: 전에는 MCP 길의 가리기를 엔진이 따로 했다).
/// 메타 파일 → 그림 → MANIFEST 차례로 써서 MANIFEST 가 가리키는 파일은 늘 있다.
#[allow(clippy::too_many_arguments)]
pub fn add_evidence(
    meta: &Meta,
    image: Option<&[u8]>,
    image_host: &str,
    url: &str,
    via: &str,
    requests: i64,
    redacted: i64,
    cost_usd: Option<f64>,
    store: &dyn EvidenceStore,
    clock: &dyn ReadClock,
) -> EvidenceWrite {
    let mut entries = manifest_parse(store.read(MANIFEST_NAME).as_deref());
    let old: Option<EvidenceEntry> = entries.iter().find(|e| e.url == url).cloned();
    let n = old.as_ref().map_or_else(|| entries.iter().map(|e| e.n).max().unwrap_or(0) + 1, |o| o.n);
    let (red, more) = meta_redact(meta);
    let file = evidence_name(n, &red.source, &red.id);
    let mut wrapped = meta_wrap_untrusted(&red); // 믿지 않는 자료 표시(절 글 감싸기·표시 괄호 바꾸기·고정 안내)
    let image_file = match image {
        None => String::new(),
        Some(_) => {
            // 원본은 dropLast(5)(`.json` 떼기). 이름이 `.json` 꼴이어서 끝 문자 다섯을 떼는 것이 안전하다
            let mut base = file.clone();
            for _ in 0..".json".len() {
                base.pop();
            }
            base.push_str(".png");
            base
        }
    };
    wrapped.image = image_file.clone();
    let checked = meta_check(Some(&meta_json(&wrapped)), true);
    let Some(ok) = checked.meta else {
        let problems = checked.problems.iter().take(3).cloned().collect::<Vec<_>>().join(" · ");
        return EvidenceWrite { entry: None, note: format!("형식 검사에 걸려 넣지 않았습니다: {problems}") };
    };
    let data = meta_data(&ok);
    let Some(sha) = store.write(&file, &data) else {
        return EvidenceWrite { entry: None, note: format!("근거 묶음에 {file} 을 쓰지 못했습니다") };
    };
    let read_at = clock.now();
    let mut e = EvidenceEntry {
        n,
        file: file.clone(),
        source: ok.source.clone(),
        via: via.to_string(),
        url: url.to_string(),
        id: ok.id.clone(),
        title: ok.title.clone(),
        read_at,
        sha256: sha,
        bytes: data.len() as i64,
        redacted: redacted + more as i64,
        requests,
        cost_usd,
        image: None,
    };
    if let Some(img) = image {
        match store.write(&image_file, img) {
            Some(ish) => {
                e.image = Some(EvidenceImage { file: image_file.clone(), host: image_host.to_string(), sha256: ish, bytes: img.len() as i64 })
            }
            None => {
                store.remove(&file);
                return EvidenceWrite { entry: None, note: format!("근거 묶음에 {image_file} 을 쓰지 못했습니다") };
            }
        }
    }
    if let Some(o) = &old {
        if o.file != file {
            store.remove(&o.file);
        }
        if let Some(oi) = &o.image {
            if oi.file != image_file {
                store.remove(&oi.file);
            }
        }
    }
    entries.retain(|x| x.n != n);
    entries.push(e.clone());
    if store.write(MANIFEST_NAME, &manifest_data(&entries, read_at)).is_none() {
        return EvidenceWrite { entry: Some(e), note: "MANIFEST.json 을 쓰지 못했습니다".to_string() };
    }
    EvidenceWrite { entry: Some(e), note: "근거 묶음에 넣었습니다".to_string() }
}
