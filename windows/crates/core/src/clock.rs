//! 시각 — 유닉스 초(소수)와 지역 시각. Python `time.time()`·`time.strftime` 대응.

use std::time::{SystemTime, UNIX_EPOCH};

/// 지금 시각(유닉스 초, 소수) — 세션 기록의 `since`·`updated`·`started` 단위.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// 지역 시각의 각 자리.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    /// `%Y%m%d-%H%M%S` — settings.json 백업 이름에 쓴다.
    pub fn stamp(&self) -> String {
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `%H:%M`
    pub fn hm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `%H:%M:%S`
    pub fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// 유닉스 초를 지역 시각으로 바꾼다. 바꾸지 못하면 UTC 로 계산한다.
pub fn local(ts: f64) -> LocalTime {
    sys::local(ts).unwrap_or_else(|| utc(ts))
}

/// UTC 로 계산(지역 시간대를 얻지 못했을 때).
fn utc(ts: f64) -> LocalTime {
    let secs = ts.floor() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // 그레고리력 날짜 계산(Howard Hinnant 의 civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    LocalTime {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
    }
}

/// ISO8601(`2026-10-16T12:00:00.123+00:00`, `…Z`)을 유닉스 초로. 시각 부분 구분자는 `T`·`t`·공백, 시간대가 없으면 None.
/// 대화 기록의 `timestamp`(Swift `isoDate`)와 사용량 API 의 `resets_at` 이 쓴다.
pub fn iso_date(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let n = |from: usize, len: usize| -> Option<i64> {
        let part = s.get(from..from + len)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok())?
    };
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, sec) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let mut i = 19;
    let mut frac = 0.0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        frac = format!("0.{}", &s[start..i]).parse().unwrap_or(0.0);
    }
    let offset = match b.get(i) {
        Some(b'Z' | b'z') if i + 1 == b.len() => 0,
        Some(&c @ (b'+' | b'-')) if b.len() == i + 6 && b[i + 3] == b':' => {
            let o = n(i + 1, 2)? * 3600 + n(i + 4, 2)? * 60;
            if c == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    // 그레고리력 날짜 → 1970-01-01 부터 날 수(Howard Hinnant 의 days_from_civil)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86400 + h * 3600 + mi * 60 + sec - offset) as f64 + frac)
}

#[cfg(unix)]
mod sys {
    use super::LocalTime;

    pub fn local(ts: f64) -> Option<LocalTime> {
        let t = ts.floor() as libc::time_t;
        // SAFETY: localtime_r 는 넘긴 tm 에만 쓴다
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
        ok.then(|| LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        })
    }
}

#[cfg(windows)]
mod sys {
    use super::LocalTime;
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    pub fn local(ts: f64) -> Option<LocalTime> {
        let ticks = crate::proc::unix_to_filetime(ts)?;
        let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let mut utc = SYSTEMTIME::default();
        let mut loc = SYSTEMTIME::default();
        // SAFETY: 넘긴 구조체에만 쓴다. 시간대 정보 null = 현재 시간대
        let ok = unsafe {
            FileTimeToSystemTime(&ft, &mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut loc) != 0
        };
        ok.then(|| LocalTime {
            year: i32::from(loc.wYear),
            month: u32::from(loc.wMonth),
            day: u32::from(loc.wDay),
            hour: u32::from(loc.wHour),
            minute: u32::from(loc.wMinute),
            second: u32::from(loc.wSecond),
        })
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    pub fn local(_ts: f64) -> Option<super::LocalTime> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        let t = utc(0.0);
        assert_eq!((t.year, t.month, t.day, t.hour), (1970, 1, 1, 0));
        // 2026-10-16 19:24:08 UTC
        let t = utc(1_792_178_648.5);
        assert_eq!(t.stamp(), "20261016-192408");
        assert_eq!(utc(951_782_400.0).stamp(), "20000229-000000");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_date("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(iso_date("2000-02-29T00:00:00+00:00"), Some(951_782_400.0));
        assert_eq!(iso_date("2026-10-16T19:24:08.5Z"), Some(1_792_178_648.5));
        assert_eq!(iso_date("2026-10-17T04:24:08+09:00"), Some(1_792_178_648.0));
        assert_eq!(iso_date("2026-10-16"), None);
        assert_eq!(iso_date("2026-10-16T19:24:08"), None);
        assert_eq!(iso_date("2026-10-16T19:24:08Zx"), None);
        assert_eq!(iso_date("가나다라마바사아자차카타파하가나다라마바사아자"), None);
    }

    #[test]
    fn local_formats() {
        let t = local(now());
        assert!(t.year >= 2024 && (1..=12).contains(&t.month));
        assert_eq!(t.hms().len(), 8);
        assert_eq!(t.hm().len(), 5);
    }
}
