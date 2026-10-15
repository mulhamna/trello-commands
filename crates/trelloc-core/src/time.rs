//! Human date input → UTC instants, Trello ISO strings, and display helpers.
//! `tz` is an IANA name (e.g. `Asia/Jakarta`); `None` or an unknown name
//! falls back to the system timezone.

use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
    Weekday,
};
use chrono_tz::Tz;

use crate::error::{Result, TrelloError};

enum Zone {
    Named(Tz),
    System,
}

fn zone(tz: Option<&str>) -> Zone {
    tz.and_then(|s| s.parse::<Tz>().ok())
        .map(Zone::Named)
        .unwrap_or(Zone::System)
}

impl Zone {
    fn local(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Zone::Named(tz) => utc.with_timezone(tz).naive_local(),
            Zone::System => utc.with_timezone(&Local).naive_local(),
        }
    }

    fn to_utc(&self, naive: NaiveDateTime) -> DateTime<Utc> {
        match self {
            Zone::Named(tz) => tz
                .from_local_datetime(&naive)
                .earliest()
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|| tz.from_utc_datetime(&naive).with_timezone(&Utc)),
            Zone::System => Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|| Local.from_utc_datetime(&naive).with_timezone(&Utc)),
        }
    }

    fn rfc3339(&self, utc: DateTime<Utc>) -> String {
        match self {
            Zone::Named(tz) => utc.with_timezone(tz).to_rfc3339(),
            Zone::System => utc.with_timezone(&Local).to_rfc3339(),
        }
    }
}

/// True for IANA names like `Asia/Jakarta` or `UTC`.
pub fn is_valid_tz(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

/// True for inputs that mean "clear the date".
pub fn is_none(input: &str) -> bool {
    matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "none" | "null" | "clear"
    )
}

/// Parse `2026-10-20`, `2026-10-20 14:00`, RFC 3339, `now`, `today`,
/// `tomorrow`, `yesterday`, `mon`..`sun` (next occurrence), `+3d`, `-2w`,
/// `+4h`, `+30m`, `eod`, `eow`.
pub fn parse_when(input: &str, tz: Option<&str>) -> Result<DateTime<Utc>> {
    parse_when_at(input, tz, Utc::now())
}

pub fn parse_when_at(input: &str, tz: Option<&str>, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let s = input.trim().to_ascii_lowercase();
    let z = zone(tz);
    let today = z.local(now).date();
    let midnight = |d: NaiveDate| z.to_utc(d.and_time(NaiveTime::MIN));
    let end_of = |d: NaiveDate| {
        z.to_utc(
            d.and_hms_milli_opt(23, 59, 59, 999)
                .expect("valid end of day"),
        )
    };
    let invalid = || {
        TrelloError::Invalid(format!(
            "cannot parse date '{input}' (try 2026-10-20, '2026-10-20 14:00', today, tomorrow, fri, +3d, eow)"
        ))
    };

    match s.as_str() {
        "" => return Err(invalid()),
        "now" => return Ok(now),
        "today" => return Ok(midnight(today)),
        "tomorrow" => return Ok(midnight(today + Duration::days(1))),
        "yesterday" => return Ok(midnight(today - Duration::days(1))),
        "eod" => return Ok(end_of(today)),
        "eow" => {
            let to_sunday = 6 - i64::from(today.weekday().num_days_from_monday());
            return Ok(end_of(today + Duration::days(to_sunday)));
        }
        _ => {}
    }

    if let Some(wd) = weekday(&s) {
        let diff = (7 + i64::from(wd.num_days_from_monday())
            - i64::from(today.weekday().num_days_from_monday()))
            % 7;
        let diff = if diff == 0 { 7 } else { diff };
        return Ok(midnight(today + Duration::days(diff)));
    }

    if let Some(sign) = s.chars().next().filter(|c| *c == '+' || *c == '-') {
        // `sign` is ASCII, so slicing after it is safe; the unit may be any
        // char, so strip it by its UTF-8 length rather than by one byte.
        let body = &s[1..];
        let unit = body.chars().last().ok_or_else(invalid)?;
        let n: i64 = body[..body.len() - unit.len_utf8()]
            .parse()
            .map_err(|_| invalid())?;
        let n = if sign == '-' { -n } else { n };
        let delta = match unit {
            'm' => Duration::try_minutes(n),
            'h' => Duration::try_hours(n),
            'd' => Duration::try_days(n),
            'w' => Duration::try_weeks(n),
            _ => return Err(invalid()),
        }
        .ok_or_else(invalid)?;
        return now.checked_add_signed(delta).ok_or_else(invalid);
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(input.trim()) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Ok(midnight(d));
    }
    for fmt in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dt%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dt%H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s, fmt) {
            return Ok(z.to_utc(dt));
        }
    }
    Err(invalid())
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "wednesday" => Weekday::Wed,
        "thu" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

/// `2026-10-20T07:00:00.000Z`, the shape Trello sends and accepts.
pub fn to_trello_iso(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Parse a human date straight to the Trello wire format.
pub fn when_iso(input: &str, tz: Option<&str>) -> Result<String> {
    parse_when(input, tz).map(to_trello_iso)
}

pub fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Creation time of any Trello object: its id's first 8 hex chars are unix
/// seconds.
pub fn created_at(id: &str) -> Option<DateTime<Utc>> {
    let secs = i64::from_str_radix(id.get(..8)?, 16).ok()?;
    DateTime::from_timestamp(secs, 0)
}

/// `2026-10-20 14:00`, or just `2026-10-20` at local midnight.
pub fn format_dt(dt: DateTime<Utc>, tz: Option<&str>) -> String {
    let local = zone(tz).local(dt);
    if local.time() == NaiveTime::MIN {
        local.format("%Y-%m-%d").to_string()
    } else {
        local.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// Display an ISO string from Trello in the configured zone.
pub fn format_iso(iso: &str, tz: Option<&str>) -> String {
    parse_iso(iso)
        .map(|d| format_dt(d, tz))
        .unwrap_or_else(|| iso.to_string())
}

/// RFC 3339 with the configured offset (for machine-readable output).
pub fn to_local_iso(dt: DateTime<Utc>, tz: Option<&str>) -> String {
    zone(tz).rfc3339(dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TZ: Option<&str> = Some("Asia/Jakarta"); // UTC+7, no DST

    fn now() -> DateTime<Utc> {
        // Wednesday 2026-10-14 10:00 in Jakarta.
        Utc.with_ymd_and_hms(2026, 10, 14, 3, 0, 0).unwrap()
    }

    fn jkt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        let tz: Tz = "Asia/Jakarta".parse().unwrap();
        tz.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn at(s: &str) -> DateTime<Utc> {
        parse_when_at(s, TZ, now()).unwrap()
    }

    #[test]
    fn absolute_dates() {
        assert_eq!(at("2026-10-20"), jkt(2026, 10, 20, 0, 0));
        assert_eq!(at("2026-10-20 14:00"), jkt(2026, 10, 20, 14, 0));
        assert_eq!(at("2026-10-20T14:30"), jkt(2026, 10, 20, 14, 30));
        assert_eq!(
            at("2026-10-20T09:00:00.000Z"),
            Utc.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap()
        );
    }

    #[test]
    fn relative_dates() {
        assert_eq!(at("today"), jkt(2026, 10, 14, 0, 0));
        assert_eq!(at("tomorrow"), jkt(2026, 10, 15, 0, 0));
        assert_eq!(at("fri"), jkt(2026, 10, 16, 0, 0));
        assert_eq!(
            at("wed"),
            jkt(2026, 10, 21, 0, 0),
            "same weekday → next week"
        );
        assert_eq!(at("+3d"), jkt(2026, 10, 17, 10, 0));
        assert_eq!(at("-2w"), jkt(2026, 9, 30, 10, 0));
        assert_eq!(
            at("eow"),
            jkt(2026, 10, 18, 23, 59) + Duration::milliseconds(59_999)
        );
        for bad in [
            "someday",
            "+",
            "-",
            "+d",
            "-é",
            "+3é",
            "+3x",
            "-6662020166202017h",
            "+9223372036854775807w",
        ] {
            assert!(parse_when_at(bad, TZ, now()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn iso_round_trip() {
        assert_eq!(
            to_trello_iso(jkt(2026, 10, 20, 0, 0)),
            "2026-10-19T17:00:00.000Z"
        );
        assert_eq!(format_iso("2026-10-19T17:00:00.000Z", TZ), "2026-10-20");
        assert_eq!(
            format_iso("2026-10-20T07:05:00.000Z", TZ),
            "2026-10-20 14:05"
        );
        assert_eq!(format_iso("garbage", TZ), "garbage");
        assert_eq!(
            to_local_iso(jkt(2026, 10, 20, 14, 5), TZ),
            "2026-10-20T14:05:00+07:00"
        );
        assert!(is_none("None") && is_none(" clear ") && !is_none("today"));
    }

    #[test]
    fn created_at_from_id() {
        // 0x5f2b0c0e = 1596656654 = 2020-08-05T19:44:14Z
        assert_eq!(
            created_at("5f2b0c0e1a2b3c4d5e6f7a8b").unwrap(),
            Utc.with_ymd_and_hms(2020, 8, 5, 19, 44, 14).unwrap()
        );
        assert!(created_at("zz").is_none());
        assert!(created_at("é1234567890").is_none());
    }

    #[test]
    fn timezones() {
        assert!(is_valid_tz("Asia/Jakarta"));
        assert!(!is_valid_tz("Mars/Olympus"));
    }
}
