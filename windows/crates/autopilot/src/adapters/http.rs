//! `Http` 어댑터 — ureq 3 + 운영체제 인증서 저장소(native-tls). 보드(`app/src-tauri/src/util.rs` 의 `http_get`)와 같은 TLS 설정이다.
//! 정책: https 만, 리다이렉트는 따르지 않는다(따르면 Authorization 헤더가 다시 나간다), 응답 본문은 4 MiB 까지,
//! 상태 코드가 2xx 가 아니어도 응답으로 돌려준다. 헤더 값은 어디에도 남기지 않는다.

use std::io;
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::ports::{Http, HttpRequest, HttpResponse, PortError, PortErrorKind, PortResult};

/// 응답 본문 상한(바이트)
pub const MAX_BODY: usize = 4 * 1024 * 1024;

pub struct UreqHttp;

impl UreqHttp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn send(&self, req: &HttpRequest) -> PortResult<HttpResponse> {
        validate_request(req)?;
        let agent = build_agent(req.timeout);

        let mut builder = ureq::http::Request::builder().method(req.method.as_str()).uri(req.url.as_str());
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let result = match &req.body {
            Some(body) => agent.run(builder.body(body.clone()).map_err(|e| PortError::new(PortErrorKind::Invalid, e.to_string()))?),
            None => agent.run(builder.body(()).map_err(|e| PortError::new(PortErrorKind::Invalid, e.to_string()))?),
        };
        let mut resp = result.map_err(|e| map_error(&e))?;

        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
            .collect();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY as u64)
            .read_to_vec()
            .map_err(|e| map_error(&e))?;
        to_response(status, headers, body)
    }
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .https_only(true)
        // 기본값(WebPki)은 OS 저장소를 끄고 Mozilla 목록만 믿어 api.anthropic.com 같은 서버에서 실패한다. 보드와 같이 OS 저장소를 쓴다
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build())
        .build()
        .into()
}

fn invalid(msg: impl Into<String>) -> PortError {
    PortError::new(PortErrorKind::Invalid, msg)
}

/// 보내기 전에 거르는 규칙(순수). 어긴 것은 `Invalid`.
pub fn validate_request(req: &HttpRequest) -> PortResult<()> {
    let scheme_ok = req.url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://"));
    if !scheme_ok {
        return Err(invalid("https:// 주소만 쓸 수 있다"));
    }
    if req.url.len() == 8 || req.url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(invalid("주소 꼴이 틀렸다"));
    }
    if req.method.is_empty() || !req.method.bytes().all(is_token_byte) {
        return Err(invalid("메서드 꼴이 틀렸다"));
    }
    for (k, v) in &req.headers {
        if k.is_empty() || !k.bytes().all(is_token_byte) {
            return Err(invalid("헤더 이름 꼴이 틀렸다"));
        }
        // 값은 로그에 남기지 않으므로 까닭에도 이름만 적는다
        if v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err(invalid(format!("헤더 {k} 의 값에 줄바꿈이나 NUL 이 있다")));
        }
    }
    Ok(())
}

/// RFC 9110 token 문자
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// 읽어 낸 응답을 포트 응답으로(순수). 상태 코드는 가리지 않고 그대로, 본문이 상한을 넘으면 `Io`.
pub fn to_response(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> PortResult<HttpResponse> {
    if body.len() > MAX_BODY {
        return Err(PortError::new(PortErrorKind::Io, "응답 본문이 4 MiB 를 넘는다"));
    }
    Ok(HttpResponse { status, headers, body })
}

/// ureq 오류를 포트 오류로(순수). 시간 초과는 `Timeout`, 연결·TLS 실패는 `Unavailable`.
pub fn map_error(e: &ureq::Error) -> PortError {
    use ureq::Error as E;
    let kind = match e {
        E::Timeout(_) => PortErrorKind::Timeout,
        E::Io(io_err) => match io_err.kind() {
            io::ErrorKind::TimedOut => PortErrorKind::Timeout,
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => PortErrorKind::Invalid,
            _ => PortErrorKind::Unavailable,
        },
        E::HostNotFound | E::ConnectionFailed | E::Tls(_) | E::NativeTls(_) | E::Der(_) | E::Pem(_) => PortErrorKind::Unavailable,
        E::BadUri(_) | E::Http(_) | E::RequireHttpsOnly(_) | E::InvalidProxyUrl => PortErrorKind::Invalid,
        E::BodyExceedsLimit(_) => PortErrorKind::Io,
        // 리다이렉트는 따르지 않도록 껐으니 여기 오면 설정이 어긋난 것이다
        _ => PortErrorKind::Io,
    };
    PortError::new(kind, e.to_string())
}
