use std::sync::LazyLock;

use regex::Regex;
use time::{Date, Month, Time, Weekday};

static DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A([0-9]{4})(-[0-9]{2}-[0-9]{2}|[0-9]{4}|-W[0-9]{2}(?:-[1-7])?|W[0-9]{2}[1-7]?)\z")
        .unwrap()
});

// Resolve the split before parsing either side, like Python's fromisoformat.
// A greedy optional weekday could consume the first digit of the clock.
fn separator_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    Some(match bytes.get(4..)? {
        [b'-', b'W', ..] => {
            // YYYY-Www-##: prefer '-' as separator when two digits follow it.
            // Otherwise '-' introduces an explicit weekday.
            if bytes.get(8) == Some(&b'-') && !bytes.get(10).is_some_and(u8::is_ascii_digit) {
                10
            } else {
                8
            }
        }
        [b'W', ..] => {
            // If the separator is itself a digit, clock components contribute
            // pairs of digits before the first colon, fraction or offset.
            let digit_count = bytes
                .get(7..)?
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            7 + if digit_count < 2 {
                digit_count
            } else {
                usize::from(digit_count.is_multiple_of(2))
            }
        }
        [b'-', ..] => 10,
        _ => 8,
    })
}

fn date(year: i32, rest: &str) -> Option<Date> {
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    if let Some(rest) = rest.strip_prefix('W') {
        let week = rest[..2].parse().ok()?;
        let suffix = rest[2..].trim_start_matches('-');
        let day = match suffix {
            "" | "1" => Weekday::Monday,
            "2" => Weekday::Tuesday,
            "3" => Weekday::Wednesday,
            "4" => Weekday::Thursday,
            "5" => Weekday::Friday,
            "6" => Weekday::Saturday,
            "7" => Weekday::Sunday,
            _ => return None,
        };
        Date::from_iso_week_date(year, week, day).ok()
    } else {
        let month = Month::try_from(rest[..2].parse::<u8>().ok()?).ok()?;
        Date::from_calendar_date(year, month, rest[2..].trim_start_matches('-').parse().ok()?).ok()
    }
}

fn clock(text: &str) -> Option<(u8, u8, u8, u32)> {
    let (whole, fraction) = text
        .split_once(['.', ','])
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let fields = match whole.as_bytes() {
        [h, i] => ([*h, *i], *b"00", *b"00", false),
        [h, i, m, n] | [h, i, b':', m, n] => ([*h, *i], [*m, *n], *b"00", false),
        [h, i, m, n, s, t] | [h, i, b':', m, n, b':', s, t] => ([*h, *i], [*m, *n], [*s, *t], true),
        _ => return None,
    };
    let pair = |[a, b]: [u8; 2]| {
        (a.is_ascii_digit() && b.is_ascii_digit()).then(|| (a - b'0') * 10 + b - b'0')
    };
    let micros = match fraction {
        None => 0,
        Some(s) if fields.3 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            let n = s.len().min(6);
            s[..n].parse::<u32>().ok()? * 10_u32.pow(6 - n as u32)
        }
        Some(_) => return None,
    };
    Some((pair(fields.0)?, pair(fields.1)?, pair(fields.2)?, micros))
}

pub(crate) fn parse(value: &str) -> Option<String> {
    let normalized = value.replace('Z', "+00:00");
    let (date_text, tail) = normalized.split_at_checked(separator_index(&normalized)?)?;
    let mut tail = tail.chars();
    tail.next()?; // Skip exactly one Unicode separator.
    let tail = tail.as_str();
    let (clock_text, offset_text) = tail.split_at(tail.find(['+', '-'])?);
    let c = DATE.captures(date_text)?;
    let year = c[1].parse().ok()?;
    if year == 0 {
        return None;
    }
    let mut date = date(year, &c[2])?;
    let (h, m, s, us) = clock(clock_text)?;
    let time = if (h, m, s, us) == (24, 0, 0, 0) {
        date = date.next_day()?;
        Time::MIDNIGHT
    } else {
        Time::from_hms_micro(h, m, s, us).ok()?
    };
    if !(1..=9999).contains(&date.year()) {
        return None;
    }
    let (sign, offset_text) = offset_text.split_at(1);
    let (oh, om, os, ous) = clock(offset_text)?;
    let offset = u32::from(oh) * 3600 + u32::from(om) * 60 + u32::from(os);
    if offset >= 86_400 {
        return None;
    }
    let sign = if offset == 0 && ous == 0 { "+" } else { sign };
    let mut out = format!(
        "{date}T{:02}:{:02}:{:02}",
        time.hour(),
        time.minute(),
        time.second()
    );
    if us != 0 {
        out.push_str(&format!(".{us:06}"));
    }
    out.push_str(&format!(
        "{sign}{:02}:{:02}",
        offset / 3600,
        offset / 60 % 60
    ));
    if offset % 60 != 0 || ous != 0 {
        out.push_str(&format!(":{:02}", offset % 60));
        if ous != 0 {
            out.push_str(&format!(".{ous:06}"));
        }
    }
    Some(out)
}
