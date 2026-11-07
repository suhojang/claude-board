// Http 어댑터 시험 — 순수 부분(요청 검증·오류 대응·응답 조립)은 늘, 진짜 TLS 한 번은 `--ignored` 로만 돈다.
use std::io;
use std::time::Duration;

use claude_board_autopilot::adapters::http::{map_error, to_response, validate_request, UreqHttp, MAX_BODY};
use claude_board_autopilot::ports::*;

fn req(url: &str) -> HttpRequest {
    HttpRequest { method: "GET".into(), url: url.into(), headers: vec![], body: None, timeout: Duration::from_secs(5) }
}

fn kind(r: PortResult<()>) -> Option<PortErrorKind> {
    r.err().map(|e| e.kind)
}

#[test]
fn https_만_통과한다() {
    assert_eq!(kind(validate_request(&req("https://api.anthropic.com/api/oauth/usage"))), None);
    assert_eq!(kind(validate_request(&req("HTTPS://example.com"))), None);
}

#[test]
fn https_가_아닌_주소는_거부한다() {
    for url in ["http://example.com", "ftp://example.com", "file:///c:/x", "example.com", "", "https://", "https", "//example.com"] {
        assert_eq!(kind(validate_request(&req(url))), Some(PortErrorKind::Invalid), "{url}");
    }
}

#[test]
fn 주소에_공백이나_제어문자가_있으면_거부한다() {
    assert_eq!(kind(validate_request(&req("https://a.com/a b"))), Some(PortErrorKind::Invalid));
    assert_eq!(kind(validate_request(&req("https://a.com/\r\nX: y"))), Some(PortErrorKind::Invalid));
}

#[test]
fn 메서드_꼴이_틀리면_거부한다() {
    for m in ["", "GE T", "GET\r\n"] {
        let mut r = req("https://a.com");
        r.method = m.into();
        assert_eq!(kind(validate_request(&r)), Some(PortErrorKind::Invalid), "{m:?}");
    }
    let mut r = req("https://a.com");
    r.method = "POST".into();
    assert_eq!(kind(validate_request(&r)), None);
}

#[test]
fn 헤더_꼴이_틀리면_거부하고_까닭에_값을_담지_않는다() {
    let mut r = req("https://a.com");
    r.headers = vec![("Authorization".into(), "Bearer SECRET\r\nX-Evil: 1".into())];
    let e = validate_request(&r).unwrap_err();
    assert_eq!(e.kind, PortErrorKind::Invalid);
    assert!(!e.message.contains("SECRET"));

    r.headers = vec![("Bad Name".into(), "v".into())];
    assert_eq!(kind(validate_request(&r)), Some(PortErrorKind::Invalid));

    r.headers = vec![("Authorization".into(), "Bearer ok".into()), ("anthropic-beta".into(), "oauth-2025-04-20".into())];
    assert_eq!(kind(validate_request(&r)), None);
}

#[test]
fn 상태_코드는_가리지_않고_그대로_돌려준다() {
    for status in [200u16, 204, 301, 401, 404, 429, 500] {
        let r = to_response(status, vec![("x".into(), "y".into())], b"body".to_vec()).unwrap();
        assert_eq!(r.status, status);
        assert_eq!(r.body, b"body");
        assert_eq!(r.headers, vec![("x".to_string(), "y".to_string())]);
    }
}

#[test]
fn 본문은_상한까지_받고_넘으면_오류다() {
    assert!(to_response(200, vec![], vec![0; MAX_BODY]).is_ok());
    let e = to_response(200, vec![], vec![0; MAX_BODY + 1]).unwrap_err();
    assert_eq!(e.kind, PortErrorKind::Io);
    assert_eq!(MAX_BODY, 4 * 1024 * 1024);
}

#[test]
fn 오류를_포트_오류로_옮긴다() {
    let k = |e: ureq::Error| map_error(&e).kind;
    assert_eq!(k(ureq::Error::Timeout(ureq::Timeout::Global)), PortErrorKind::Timeout);
    assert_eq!(k(ureq::Error::Io(io::Error::from(io::ErrorKind::TimedOut))), PortErrorKind::Timeout);
    assert_eq!(k(ureq::Error::Io(io::Error::from(io::ErrorKind::ConnectionRefused))), PortErrorKind::Unavailable);
    assert_eq!(k(ureq::Error::HostNotFound), PortErrorKind::Unavailable);
    assert_eq!(k(ureq::Error::ConnectionFailed), PortErrorKind::Unavailable);
    assert_eq!(k(ureq::Error::Tls("handshake")), PortErrorKind::Unavailable);
    assert_eq!(k(ureq::Error::BadUri("x".into())), PortErrorKind::Invalid);
    assert_eq!(k(ureq::Error::BodyExceedsLimit(4)), PortErrorKind::Io);
}

#[test]
fn 어댑터도_http_주소를_보내기_전에_거부한다() {
    let e = UreqHttp::new().send(&req("http://127.0.0.1:1/")).unwrap_err();
    assert_eq!(e.kind, PortErrorKind::Invalid);
}

/// 이 PC 의 OS 인증서 저장소로 진짜 TLS 가 되는지 — `cargo test --test http -- --ignored`
#[test]
#[ignore = "네트워크가 필요하다"]
fn 진짜_서버에_tls_로_닿는다() {
    let mut r = req("https://api.anthropic.com/");
    r.timeout = Duration::from_secs(20);
    let resp = UreqHttp::new().send(&r).expect("TLS 연결이 되어야 한다");
    eprintln!("status = {}", resp.status);
    assert!((100..600).contains(&resp.status));
}
