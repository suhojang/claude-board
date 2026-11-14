//! 경로를 「같은 자리인가」 견줄 꼴로 접는다. 원본: `Sources/AutopilotRules/Critique.swift` 의 `apPathKey`(⑪-3 자동 보안 검토 MEDIUM).
//! 쓰는 곳: Multirun(`apPathOverlap`·`apClaimKeys`: 같은 경로 자리), Takeover, Harness(보호 자리·정렬·지문), Critique(보호 판정 자리).
//!
//! 두 함수가 있다.
//! - `path_key` — Swift `apPathKey` 와 같다. 「/」로 조각을 나누고 조각마다 유니코드 정규화(NFC)를 한 뒤 소문자로 접어 다시 「/」로 잇는다.
//!   그 밖은 건드리지 않는다(빈 조각·끝의 「/」·`.`·`..` 가 그대로 남는다). macOS 와 같은 입력은 같은 출력이다. **Windows 에서 쓰면 안 된다.**
//! - `path_key_windows` — **Windows 엔진(호출하는 쪽)이 쓸 것.** `path_key` 위에 Windows 가 같은 폴더로 받아 주는 다른 쓰기를 한 꼴로 모은다.
//!   `\` 를 「/」로, `\\?\`·`\\.\`·`\??\` 앞붙이를 떼고(`\\?\UNC\서버\공유` 는 `//서버/공유`), 조각 끝의 점·공백을 떼고, 반복된 구분자를 하나로,
//!   `.`·`..` 를 글자 위에서만 풀고, NTFS 대체 데이터 스트림(`a.txt::$DATA`·`a:스트림`)의 `:` 뒤를 떼고 소문자로 접는다.
//!   더 느슨해지는 쪽(서로 다른 쓰기를 다른 자리로 세어 두 실행이 같은 자리를 잡는 것)이 보안 위험이므로 애매하면 더 많이 합친다.
//!   결과 꼴: 드라이브 `c:/a/b`(루트는 `c:`), UNC `//서버/공유/a`, 드라이브 없는 절대 `/a`(루트는 `/`), 드라이브 상대 `c:a`, 상대 `a/b`(앞쪽 `..` 는 남김).
//!   끝에 「/」가 없다. 두 번 불러도 같은 값(멱등)이다.
//!
//! 이 함수가 못 하는 것(어댑터의 실제 경로 조회가 먼저 해야 한다 — Multirun 의 `apRealPath` 자리):
//! - 8.3 짧은 이름(`PROGRA~1` = `Program Files`)은 글자만으로 풀 수 없다. 어댑터가 `GetLongPathNameW`(또는 `std::fs::canonicalize`)로 긴 이름을 먼저 얻는다.
//! - 정션·심볼릭 링크·`subst` 드라이브·공유 마운트는 어댑터가 실제 경로(`canonicalize` 후 `\\?\` 앞붙이 떼기)로 푼 뒤에 이 키를 만든다.
//! - 예약 장치 이름(`CON`·`NUL`·`COM1` 등)과 `/` 가 아닌 다른 유니코드 구분자는 다루지 않는다.
//!
//! Swift ↔ Rust 로 달라진 점(`path_key`):
//! - Swift 의 `split(separator: "/")` 은 글자 묶음(Character) 단위라 「/」 뒤에 결합 부호가 붙으면(`"/\u{301}"`) 한 묶음이 되어 나뉘지 않는다. 여기는 코드 단위(`'/'`)로 나눠 나눈다.
//!   실제 경로 이름에서 구분자 바로 뒤에 결합 부호만 오는 일은 없고, 나뉘는 쪽이 더 잘게 견주므로 느슨해지지 않는다.
//! - Swift `lowercased()` 는 글자마다 소문자 사상(`İ` → `i̇` 같은 한 글자→여러 글자 포함, 문맥 없음)이다. 여기는 `char::to_lowercase` 를 글자마다 이어 같은 뜻이다
//!   (`str::to_lowercase` 는 마지막 시그마 `Σ`→`ς` 문맥 규칙이 있어 쓰지 않는다).
//! - Swift `precomposedStringWithCanonicalMapping`(Foundation NFC)과 `unicode-normalization` 의 `nfc()` 는 둘 다 유니코드 표준 NFC 라 같다
//!   (유니코드 버전이 다르면 아주 최근에 추가된 글자에서 다를 수 있다).

use unicode_normalization::UnicodeNormalization;

/// 한 조각을 NFC 로 합친 뒤 글자마다 소문자로 접는다(Swift `.precomposedStringWithCanonicalMapping.lowercased()`).
fn fold_piece(piece: &str) -> String {
    piece.nfc().flat_map(char::to_lowercase).collect()
}

/// 경로를 견줄 꼴로(Swift `apPathKey`): 「/」 로 나눠 조각마다 NFC·소문자, 다시 「/」 로 잇는다. 빈 조각과 끝의 「/」 는 그대로 남는다.
/// Windows 엔진은 이것이 아니라 [`path_key_windows`] 를 쓴다.
pub fn path_key(p: &str) -> String {
    p.split('/').map(fold_piece).collect::<Vec<_>>().join("/")
}

/// 앞붙이를 뗀 경로의 어디에 뿌리를 둘까.
enum Root {
    /// `//서버/공유/…` (UNC). 서버·공유 두 조각 아래로는 `..` 가 올라가지 않는다
    Unc,
    /// `/…` (드라이브 없는 절대)
    Rooted,
    /// `c:/…` (드라이브 절대). 드라이브 문자는 이미 `c:` 로 소문자
    Drive(String),
    /// `c:…` (드라이브 상대, `c:` 뒤에 구분자 없음)
    DriveRelative(String),
    Relative,
}

/// Windows 가 같은 자리로 받아 주는 쓰기를 한 꼴로 모은 경로 키 — **Windows 엔진이 쓰는 함수.** 자세한 규칙은 파일 머리 설명.
pub fn path_key_windows(p: &str) -> String {
    let folded = path_key(&p.replace('\\', "/"));
    let (root, rest) = split_root(&folded);
    let floor = if matches!(root, Root::Unc) { 2 } else { 0 };
    let absolute = !matches!(root, Root::Relative | Root::DriveRelative(_));
    let mut out: Vec<String> = Vec::new();
    for piece in rest.split('/') {
        match clean_piece(piece) {
            None => {}
            Some(c) if c == "." => {}
            Some(c) if c == ".." => {
                if out.len() > floor && out.last().is_some_and(|l| l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..".to_string());
                }
                // 절대 경로의 뿌리 위로는 올라가지 못한다(글자 위에서 뿌리에 머문다)
            }
            Some(c) => out.push(c),
        }
    }
    let body = out.join("/");
    match root {
        Root::Unc => format!("//{body}"),
        Root::Rooted => format!("/{body}"),
        Root::Drive(d) if body.is_empty() => d,
        Root::Drive(d) => format!("{d}/{body}"),
        Root::DriveRelative(d) => format!("{d}{body}"),
        Root::Relative => body,
    }
}

/// 앞붙이(`//?/`·`//./`·`/??/`)를 떼고 뿌리 종류와 그 뒤 남은 글자를 가른다. 입력은 이미 `/` 로 통일되고 접힌 꼴이다.
fn split_root(s: &str) -> (Root, String) {
    let mut s = s;
    let mut had_prefix = false;
    for pre in ["//?/", "//./", "/??/"] {
        if let Some(r) = s.strip_prefix(pre) {
            s = r;
            had_prefix = true;
            break;
        }
    }
    if had_prefix {
        // `\\?\UNC\서버\공유` 는 UNC, `\\?\C:\x` 는 드라이브 절대
        if let Some(r) = s.strip_prefix("unc/") {
            return (Root::Unc, r.to_string());
        }
        // 앞붙이 뒤는 드라이브나 볼륨 이름이다. 아래 분류를 그대로 따른다(앞붙이는 절대 경로를 뜻하므로 상대로 두지 않는다)
        if let Some((drive, rest)) = drive_of(s) {
            return (Root::Drive(drive), rest.trim_start_matches('/').to_string());
        }
        return (Root::Rooted, s.to_string());
    }
    if let Some(r) = s.strip_prefix("//") {
        return (Root::Unc, r.to_string());
    }
    if s.starts_with('/') {
        return (Root::Rooted, s.to_string());
    }
    if let Some((drive, rest)) = drive_of(s) {
        return if rest.starts_with('/') {
            (Root::Drive(drive), rest.to_string())
        } else {
            (Root::DriveRelative(drive), rest.to_string())
        };
    }
    (Root::Relative, s.to_string())
}

/// `c:` 로 시작하면 (`c:`, 그 뒤). 접힌 소문자 ASCII 알파벳 한 글자만 드라이브 문자다.
fn drive_of(s: &str) -> Option<(String, &str)> {
    let mut it = s.chars();
    let c = it.next()?;
    if c.is_ascii_lowercase() && it.next() == Some(':') {
        return Some((format!("{c}:"), &s[2..]));
    }
    None
}

/// 조각 하나를 Windows 가 같은 이름으로 보는 꼴로 줄인다. 버릴 조각이면 `None`(빈 조각·점과 공백뿐인 조각).
fn clean_piece(piece: &str) -> Option<String> {
    // 대체 데이터 스트림(`a.txt:스트림`·`a.txt::$data`·`dir::$index_allocation`)은 본 이름과 같은 자리다
    let base = piece.split(':').next().unwrap_or("");
    let t = base.trim_end_matches(' ');
    if t == "." || t == ".." {
        return Some(t.to_string());
    }
    let t = t.trim_end_matches(['.', ' ']);
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
