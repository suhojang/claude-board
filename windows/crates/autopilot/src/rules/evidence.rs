//! 근거 묶음과 `MANIFEST.json`(원본 `Sources/AutopilotRules/Evidence.swift`, 설계 5장 6절).
//! 읽은 것 하나가 메타 파일 하나(`<n>-<출처>-<id>.json`)와 Figma 면 그림 한 장(`<n>-figma-<id>.png`)이고, MANIFEST 가 그 목록이다.
//! 같은 주소를 다시 읽으면 같은 번호의 파일을 바꾼다. 파일 쓰기(임시 + rename, 읽기 전용)와 해시는 어댑터 몫이다.
//!
//! 이 파일의 이름은 모두 묶음 폴더 안의 맨 파일 이름(`&str`)이다. 경로 구분자(`/`·`\`)나 `..` 는 이름 검사가 거른다.
//! 폴더 경로 합치기·정규화는 부르는 쪽 몫이고, 여기서는 파일 시스템을 부르지 않는다.
//!
//! 옮기지 않은 것: `apAddEvidence`(메타 가리기·형식 검사·저장소·시계를 엮는 문). 그 재료(`apMeta*`, `APEvidenceStore`, `APClock`)가
//! 이 crate 의 규칙 파일에 없어, 엔진이 이 파일의 번호·이름·MANIFEST 규칙을 써서 직접 엮는다.

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
                ("read_at", Value::from(e.read_at)),
                ("sha256", Value::from(e.sha256.as_str())),
                ("bytes", Value::from(e.bytes)),
                ("redacted", Value::from(e.redacted)),
                ("requests", Value::from(e.requests)),
            ];
            if let Some(c) = e.cost_usd {
                d.push(("cost_usd", Value::from(c)));
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
        ("updated", Value::from(updated)),
        ("redacted", Value::from(entries.iter().map(|e| e.redacted).sum::<i64>())),
        ("entries", Value::Array(list)),
    ])
}

/// 원본: `apManifestData`. 들여쓰기 2칸·키 정렬·슬래시 이스케이프 없음.
/// Swift 의 `"키" : 값` 과 달리 `"키": 값` 으로 쓴다(읽는 쪽은 JSON 이라 같다).
pub fn manifest_data(entries: &[EvidenceEntry], updated: f64) -> Vec<u8> {
    serde_json::to_vec_pretty(&manifest_json(entries, updated)).unwrap_or_default()
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
