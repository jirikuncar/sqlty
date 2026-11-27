//! Text PostgreSQL rejects as the input of a type, as its input functions (`int4in`,
//! `numeric_in`, `float8in`, `boolin`, `uuid_in`, `date_in`, ...) read it. Each check
//! accepts at least what the server does: date, time and interval text is rejected only
//! when it cannot hold a value at all, or is an ISO date or time out of range.

/// The name PostgreSQL gives `sql_type` in its error, when it rejects `text` as one.
pub fn rejected(sql_type: &str, text: &str) -> Option<&'static str> {
    if text.contains('\0') {
        return None;
    }
    let base = sql_type.split('(').next().unwrap_or(sql_type).trim_end();
    let (name, valid) = match base {
        "SMALLINT" => ("smallint", integer(text, i16::MIN.into(), i16::MAX.into())),
        "INT" => ("integer", integer(text, i32::MIN.into(), i32::MAX.into())),
        "BIGINT" => ("bigint", integer(text, i64::MIN.into(), i64::MAX.into())),
        "DECIMAL" | "NUMERIC" => ("numeric", numeric(text)),
        "FLOAT" => ("real", float(text)),
        "DOUBLE" => ("double precision", float(text)),
        "BOOLEAN" => ("boolean", boolean(text)),
        "UUID" => ("uuid", uuid(text)),
        "DATE" => ("date", datetime(text, date)),
        "TIME" => ("time", datetime(text, time)),
        "TIMETZ" => ("time with time zone", datetime(text, |_| None)),
        "TIMESTAMP" => ("timestamp", datetime(text, timestamp)),
        "TIMESTAMPTZ" => ("timestamp with time zone", datetime(text, timestamp)),
        "INTERVAL" => ("interval", interval(text)),
        _ => return None,
    };
    (!valid).then_some(name)
}

/// What `isspace` skips around a value.
fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
}

/// `[+-]` then the digits of `rest`, stripped.
fn signed(text: &str) -> (bool, &str) {
    match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    }
}

/// The value of digits in `radix` with single `_` between them, as `pg_strtoint64` reads
/// them; `None` when they are not, saturated past `u128`.
fn digits(text: &str, radix: u32, underscore_first: bool) -> Option<u128> {
    let mut value: u128 = 0;
    let mut any = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if !any && !underscore_first {
                return None;
            }
            chars.peek().filter(|next| next.is_digit(radix))?;
            continue;
        }
        let digit = c.to_digit(radix)?;
        value = value
            .saturating_mul(radix.into())
            .saturating_add(digit.into());
        any = true;
    }
    any.then_some(value)
}

/// The radix and digits of `0x`, `0o` and `0b` integers.
fn prefixed(text: &str) -> Option<(u32, &str)> {
    let radix = match text.get(..2)? {
        "0x" | "0X" => 16,
        "0o" | "0O" => 8,
        "0b" | "0B" => 2,
        _ => return None,
    };
    Some((radix, &text[2..]))
}

fn integer(text: &str, min: i128, max: i128) -> bool {
    let (negative, rest) = signed(trim(text));
    let value = match prefixed(rest) {
        Some((radix, rest)) => digits(rest, radix, true),
        None => digits(rest, 10, false),
    };
    value
        .and_then(|value| i128::try_from(value).ok())
        .map(|value| if negative { -value } else { value })
        .is_some_and(|value| (min..=max).contains(&value))
}

fn special(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.eq_ignore_ascii_case(word))
}

/// Decimal digits with an optional fraction and exponent: `1_000.5e-3`, `.5`, `5.`.
fn decimal(text: &str, underscores: bool) -> bool {
    let ok = |part: &str| {
        if underscores {
            digits(part, 10, false).is_some()
        } else {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
        }
    };
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(signed(&text[at + 1..]).1)),
        None => (text, None),
    };
    let fits = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            (whole.is_empty() || ok(whole))
                && (fraction.is_empty() || ok(fraction))
                && !(whole.is_empty() && fraction.is_empty())
        }
        None => ok(mantissa),
    };
    fits && exponent.is_none_or(ok)
}

fn numeric(text: &str) -> bool {
    let text = trim(text);
    let (_, rest) = signed(text);
    special(text, &["nan"])
        || special(rest, &["infinity", "inf"])
        || match prefixed(rest) {
            Some((radix, rest)) => digits(rest, radix, true).is_some(),
            None => decimal(rest, true),
        }
}

/// What `strtod` reads, whole: decimal and hexadecimal floats, `inf`, `infinity` and `nan`.
fn float(text: &str) -> bool {
    let (_, rest) = signed(trim(text));
    if special(rest, &["infinity", "inf", "nan"])
        || rest
            .get(..4)
            .is_some_and(|nan| nan.eq_ignore_ascii_case("nan("))
            && rest.ends_with(')')
            && rest[4..rest.len() - 1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return true;
    }
    match rest.get(..2) {
        Some("0x" | "0X") => {
            let hex = &rest[2..];
            let (mantissa, exponent) = match hex.find(['p', 'P']) {
                Some(at) => (&hex[..at], Some(signed(&hex[at + 1..]).1)),
                None => (hex, None),
            };
            let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
            let hex_digits = |part: &str| part.bytes().all(|b| b.is_ascii_hexdigit());
            hex_digits(whole)
                && hex_digits(fraction)
                && !(whole.is_empty() && fraction.is_empty())
                && exponent.is_none_or(|e| !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit()))
        }
        _ => decimal(rest, false),
    }
}

/// `parse_bool_with_len`: a prefix of `true`, `false`, `yes` or `no`, `on`, `off` (two
/// letters at least), `1` or `0`, in any case.
fn boolean(text: &str) -> bool {
    let text = trim(text).to_ascii_lowercase();
    let prefix = |word: &str, least: usize| text.len() >= least && word.starts_with(&text);
    prefix("true", 1)
        || prefix("false", 1)
        || prefix("yes", 1)
        || prefix("no", 1)
        || prefix("on", 2)
        || prefix("off", 2)
        || text == "1"
        || text == "0"
}

/// `string_to_uuid`: 32 hex digits, a `-` allowed after each group of four but the last,
/// in optional braces; no spaces.
fn uuid(text: &str) -> bool {
    let (braced, mut rest) = match text.strip_prefix('{') {
        Some(rest) => (true, rest.as_bytes()),
        None => (false, text.as_bytes()),
    };
    for byte in 0..16 {
        match rest {
            [a, b, tail @ ..] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => rest = tail,
            _ => return false,
        }
        if byte % 2 == 1 && byte < 15 {
            if let [b'-', tail @ ..] = rest {
                rest = tail;
            }
        }
    }
    if braced {
        match rest {
            [b'}', tail @ ..] => rest = tail,
            _ => return false,
        }
    }
    rest.is_empty()
}

/// The words that stand for a date or time on their own.
const SPECIAL: [&str; 7] = [
    "epoch",
    "infinity",
    "now",
    "today",
    "tomorrow",
    "yesterday",
    "allballs",
];

/// Date and time text: rejected when it is blank, has no digit and no word of
/// [`SPECIAL`], or reads as `iso` but is out of range.
fn datetime(text: &str, iso: fn(&str) -> Option<bool>) -> bool {
    let text = trim(text);
    if text.is_empty() {
        return false;
    }
    if !text.bytes().any(|b| b.is_ascii_digit()) {
        return text
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|word| special(word, &SPECIAL));
    }
    iso(text).unwrap_or(true)
}

/// `n` ASCII digits as a number.
fn number(text: &str) -> Option<u32> {
    text.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| text.parse().ok())
        .flatten()
}

/// Whether `YYYY-MM-DD` is a day of the calendar; `None` when `text` is not of that form.
fn date(text: &str) -> Option<bool> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let (year, month, day) = (
        number(&text[..4])?,
        number(&text[5..7])?,
        number(&text[8..])?,
    );
    if year == 0 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Some(false),
    };
    Some((1..=days).contains(&day))
}

/// Whether `HH:MM[:SS.ffffff]` is a time of day, `24:00:00` included; `None` when
/// `text` is not of that form.
fn time(text: &str) -> Option<bool> {
    let (clock, fraction) = text.split_once('.').unwrap_or((text, "0"));
    if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<u32> = clock
        .split(':')
        .map(|part| number(part).filter(|_| part.len() == 2))
        .collect::<Option<_>>()?;
    // `MM:SS.f` reads as minutes and seconds.
    if !(2..=3).contains(&parts.len()) || parts.len() == 2 && text.contains('.') {
        return None;
    }
    let (hour, minute, second) = (parts[0], parts[1], parts.get(2).copied().unwrap_or(0));
    let midnight = hour == 24 && minute == 0 && second == 0 && fraction.bytes().all(|b| b == b'0');
    Some(midnight || (hour < 24 && minute < 60 && second <= 60))
}

/// `YYYY-MM-DD`, or that and a [`time`] after a space or `T`.
fn timestamp(text: &str) -> Option<bool> {
    match text.get(10..11) {
        None => date(text),
        Some(" " | "T") => Some(date(&text[..10])? && time(&text[11..])?),
        Some(_) => None,
    }
}

/// Interval text: rejected when blank, or without a digit, a `.`, a leading ISO `P` and
/// the word `infinity`.
fn interval(text: &str) -> bool {
    let text = trim(text);
    !text.is_empty()
        && (text.bytes().any(|b| b.is_ascii_digit() || b == b'.')
            || text.starts_with(['P', 'p'])
            || text
                .split(|c: char| !c.is_ascii_alphabetic())
                .any(|word| special(word, &["infinity"])))
}
