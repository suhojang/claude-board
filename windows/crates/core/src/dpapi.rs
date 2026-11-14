//! DPAPI(현재 사용자) 봉인·풀기 — 계정 금고와 자율주행 비밀 저장이 함께 쓴다.
//!
//! Windows 에서만 동작하고 그 밖의 OS 에서는 항상 `Err(Unsupported)` 다(평문 대체는 부르는 쪽이 정한다).
//! `protect`·`unprotect` 는 엔트로피·설명 문자열을 쓰지 않는다(계정 금고). `*_with_entropy` 는 호출자가 준 엔트로피를 더해
//! 같은 사용자라도 다른 엔트로피로 봉인한 것은 못 풀게 한다(자율주행 비밀 저장). 둘 다 `CRYPTPROTECT_UI_FORBIDDEN` 으로 화면 확인을 막는다.

use std::io;

/// 바이트를 DPAPI 로 봉인한다. 빈 입력도 봉인된다.
pub fn protect(plain: &[u8]) -> io::Result<Vec<u8>> {
    sys::protect(plain, None)
}

/// 엔트로피를 더해 봉인한다. 엔트로피가 비어 있으면 `InvalidInput`(엔트로피 없는 봉인은 `protect` 다).
pub fn protect_with_entropy(plain: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
    check_entropy(entropy)?;
    sys::protect(plain, Some(entropy))
}

/// DPAPI 로 봉인한 바이트를 푼다. DPAPI 가 만든 것이 아니거나 망가졌으면 `Err`.
pub fn unprotect(sealed: &[u8]) -> io::Result<Vec<u8>> {
    sys::unprotect(sealed, None)
}

/// 같은 엔트로피로 봉인한 바이트를 푼다. 엔트로피가 다르거나(엔트로피 없이 봉인한 것 포함) 망가졌으면 `Err`.
pub fn unprotect_with_entropy(sealed: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
    check_entropy(entropy)?;
    sys::unprotect(sealed, Some(entropy))
}

fn check_entropy(entropy: &[u8]) -> io::Result<()> {
    if entropy.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "엔트로피가 비어 있다"));
    }
    Ok(())
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// DPAPI 결과를 복사하고 LocalFree 로 놓는다(빈 결과는 포인터가 null 일 수 있다).
    ///
    /// # Safety
    /// `out` 은 DPAPI 가 채운 값이어야 한다.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI 가 채운 버퍼와 길이
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData.cast()) };
        v
    }

    pub fn protect(plain: &[u8], entropy: Option<&[u8]>) -> io::Result<Vec<u8>> {
        let input = blob(plain);
        let ent = entropy.map(blob);
        let ent_ptr = ent.as_ref().map_or(null(), |e| e as *const CRYPT_INTEGER_BLOB);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: 입력·엔트로피 버퍼는 이 함수 동안 살아 있고, 출력은 take 가 놓는다
        let ok = unsafe { CryptProtectData(&input, null(), ent_ptr, null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) } != 0;
        if ok {
            Ok(unsafe { take(out) })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn unprotect(sealed: &[u8], entropy: Option<&[u8]>) -> io::Result<Vec<u8>> {
        // DPAPI 봉인은 비어 있을 수 없다. 빈 조각의 포인터는 값이 아무 주소(dangling)라 CryptUnprotectData 가
        // 가끔 성공으로 답하므로(실측: 같은 시험이 드물게 Some([]) 을 냈다) 부르기 전에 거절한다.
        if sealed.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "빈 입력은 DPAPI 봉인이 아니다"));
        }
        let input = blob(sealed);
        let ent = entropy.map(blob);
        let ent_ptr = ent.as_ref().map_or(null(), |e| e as *const CRYPT_INTEGER_BLOB);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        // SAFETY: 위와 같다
        let ok = unsafe { CryptUnprotectData(&input, null_mut(), ent_ptr, null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) } != 0;
        if ok {
            Ok(unsafe { take(out) })
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use std::io;

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "DPAPI 는 Windows 에서만 쓸 수 있다")
    }

    pub fn protect(_plain: &[u8], _entropy: Option<&[u8]>) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }

    pub fn unprotect(_sealed: &[u8], _entropy: Option<&[u8]>) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }
}
