use time::{Date, Month, PrimitiveDateTime, Time, Weekday};

const SECOND: i64 = 1_000_000;
pub(crate) const DAY: i64 = 86_400 * SECOND;

#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    local: PrimitiveDateTime,
    offset: i64,
}

impl Stamp {
    pub(crate) fn instant(self) -> i64 {
        self.local.assume_utc().unix_timestamp() * SECOND + i64::from(self.local.microsecond())
            - self.offset
    }

    pub(crate) fn isoformat(self) -> String {
        let dt = self.local;
        let mut text = format!(
            "{}T{:02}:{:02}:{:02}",
            dt.date(),
            dt.hour(),
            dt.minute(),
            dt.second()
        );
        if dt.microsecond() != 0 {
            text.push_str(&format!(".{:06}", dt.microsecond()));
        }
        let offset = self.offset.abs();
        let whole = offset / SECOND;
        let sign = if self.offset < 0 { '-' } else { '+' };
        text.push_str(&format!("{sign}{:02}:{:02}", whole / 3600, whole / 60 % 60));
        if whole % 60 != 0 || offset % SECOND != 0 {
            text.push_str(&format!(":{:02}", whole % 60));
            if offset % SECOND != 0 {
                text.push_str(&format!(".{:06}", offset % SECOND));
            }
        }
        text
    }

    pub(crate) fn window_start_is_valid(self) -> bool {
        self.local.date().to_julian_day() - 90
            >= Date::from_calendar_date(1, Month::January, 1)
                .unwrap()
                .to_julian_day()
    }
}

fn digits(text: &mut &str, n: usize) -> Option<u32> {
    let prefix = text.get(..n)?;
    if !prefix.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    *text = &text[n..];
    prefix.parse().ok()
}

fn date(text: &mut &str) -> Option<Date> {
    let year = i32::try_from(digits(text, 4)?).ok()?;
    if year == 0 {
        return None;
    }
    let extended = text.starts_with('-');
    if extended {
        *text = &text[1..];
    }
    if let Some(rest) = text.strip_prefix('W') {
        *text = rest;
        let week = u8::try_from(digits(text, 2)?).ok()?;
        let weekday = if extended {
            // A hyphen followed by a digit is the weekday delimiter.
            if text.as_bytes().get(1).is_some_and(u8::is_ascii_digit) && text.starts_with('-') {
                *text = &text[1..];
                digits(text, 1)?
            } else {
                1
            }
        } else if text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            digits(text, 1)?
        } else {
            1
        };
        let weekday = [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
        ]
        .get(weekday.checked_sub(1)? as usize)?;
        let day = Date::from_iso_week_date(year, week, *weekday).ok()?;
        return (1..=9999).contains(&day.year()).then_some(day);
    }
    let month = Month::try_from(u8::try_from(digits(text, 2)?).ok()?).ok()?;
    if extended {
        *text = text.strip_prefix('-')?;
    }
    Date::from_calendar_date(year, month, u8::try_from(digits(text, 2)?).ok()?).ok()
}

// Python accepts basic/extended clocks, with optional minutes, seconds and
// decimal fractions. Offsets reuse the clock grammar but normalize components.
fn clock(text: &mut &str) -> Option<(u8, u8, u8, u32)> {
    let hour = u8::try_from(digits(text, 2)?).ok()?;
    let mut minute = 0;
    let mut second = 0;
    let colon = text.starts_with(':');
    let mut has_seconds = false;
    if colon || text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        if colon {
            *text = &text[1..];
        }
        minute = u8::try_from(digits(text, 2)?).ok()?;
        if colon && text.starts_with(':')
            || !colon && text.as_bytes().first().is_some_and(u8::is_ascii_digit)
        {
            if colon {
                *text = &text[1..];
            }
            second = u8::try_from(digits(text, 2)?).ok()?;
            has_seconds = true;
        }
    }
    let mut micros = 0;
    if let Some(rest) = text.strip_prefix(['.', ',']) {
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 || !has_seconds {
            return None;
        }
        let precision = n.min(6);
        micros = rest[..precision].parse::<u32>().ok()? * 10_u32.pow((6 - precision) as u32);
        *text = &rest[n..];
    }
    Some((hour, minute, second, micros))
}

pub(crate) fn parse(input: &str) -> Option<Stamp> {
    let replaced = input.replace('Z', "+00:00");
    let mut text = replaced.as_str();
    let mut date = date(&mut text)?;
    if text.is_empty() {
        return Some(Stamp {
            local: date.midnight(),
            offset: 0,
        });
    }
    // fromisoformat allows any single Unicode separator.
    let mut chars = text.chars();
    chars.next()?;
    text = chars.as_str();
    let (h, m, s, us) = clock(&mut text)?;
    let time = if (h, m, s, us) == (24, 0, 0, 0) {
        date = date.next_day().filter(|d| d.year() <= 9999)?;
        Time::MIDNIGHT
    } else {
        Time::from_hms_micro(h, m, s, us).ok()?
    };
    let offset = if let Some(rest) = text.strip_prefix(['+', '-']) {
        let sign = if text.starts_with('-') { -1 } else { 1 };
        text = rest;
        let (h, m, s, us) = clock(&mut text)?;
        let whole = i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s);
        let offset = whole * SECOND + i64::from(us);
        if offset >= DAY {
            return None;
        }
        sign * offset
    } else {
        0
    };
    text.is_empty().then_some(Stamp {
        local: date.with_time(time),
        offset,
    })
}
