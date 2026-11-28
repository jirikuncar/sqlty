//! Text PostgreSQL rejects for a type, as `tests/test_param_text.py` checks it against a
//! server: every rejected case here is one the server rejects too.

use sqlty_python::input::rejected;

#[test]
fn accepted() {
    let cases = [
        (
            "SMALLINT",
            &["1", " -32768 ", "+0x7FFF", "0o17", "0b1_01", "1_000"][..],
        ),
        ("INT", &["0x_1", "2147483647"]),
        ("BIGINT", &["-9223372036854775808"]),
        (
            "DECIMAL(10, 2)",
            &[
                "1_000.5",
                ".5",
                "5.",
                "1e3",
                "-1.5E-3",
                "NaN",
                "-inf",
                "+Infinity",
                "0x1F",
            ],
        ),
        (
            "FLOAT",
            &["1e3", "inf", "nan(123)", "0x1p3", "0x.8", "0xA.P-2"],
        ),
        ("DOUBLE", &["-.5", " 2 "]),
        (
            "BOOLEAN",
            &["t", "TRUE", " yes ", "of", "on", "n", "1", "0"],
        ),
        (
            "UUID",
            &[
                "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
                "{A0EEBC999C0B4EF8BB6D6BB9BD380A11}",
                "a0ee-bc99-9c0b-4ef8-bb6d-6bb9-bd38-0a11",
            ],
        ),
        (
            "DATE",
            &[
                "2024-02-29",
                "today",
                "January 8, 99 BC",
                "0000-01-01",
                "2026-11-26 Japan",
            ],
        ),
        (
            "TIME",
            &[
                "allballs",
                "23:59:60",
                "24:00:00.000",
                "12:30:00.123456",
                "1:30",
                "12:30:",
                "12:30.",
                "12:30.x",
                "12:30:00:00",
                "30:00.5",
            ],
        ),
        ("TIMETZ", &["now"]),
        (
            "TIMESTAMP",
            &[
                "2026-11-26T07:00:00.5",
                "2026-11-26 24:00",
                "epoch",
                "2026-11-26X",
            ],
        ),
        ("TIMESTAMPTZ", &["-infinity", "2026-11-26 07:00 UTC"]),
        (
            "INTERVAL",
            &["1 day 2 hours ago", "PT", ".", "infinity", "@ 1 minute"],
        ),
        ("TEXT", &["anything"]),
    ];
    for (sql_type, texts) in cases {
        for text in texts {
            assert_eq!(rejected(sql_type, text), None, "{sql_type} {text:?}");
        }
    }
    assert_eq!(rejected("INT", "a\0"), None, "PostgreSQL text holds no NUL");
}

#[test]
fn rejected_texts() {
    let cases = [
        (
            "SMALLINT",
            "smallint",
            &["", "abc", "32768", "_1", "1__0", "1_", "0x", "1.5", "--1"][..],
        ),
        ("INT", "integer", &["2147483648", "0xg"]),
        ("BIGINT", "bigint", &["9223372036854775808", "1e3"]),
        (
            "DECIMAL",
            "numeric",
            &["1,50", ".", "1e", "e1", "0x1p3", "1.2.3"],
        ),
        (
            "FLOAT",
            "real",
            &["1_0.5", "high", "0x", "0x1p", "nan(", "0b1", "abcé"],
        ),
        ("DOUBLE", "double precision", &["1,5"]),
        ("BOOLEAN", "boolean", &["o", "", "nope", "2"]),
        (
            "UUID",
            "uuid",
            &[
                "a0eebc99",
                " a0eebc999c0b4ef8bb6d6bb9bd380a11",
                "{a0eebc999c0b4ef8bb6d6bb9bd380a11",
                "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11-",
                "g0eebc999c0b4ef8bb6d6bb9bd380a11",
            ],
        ),
        (
            "DATE",
            "date",
            &[
                "",
                " ",
                "not a date",
                "Japan",
                "2026-02-29",
                "2026-13-01",
                "2026-04-31",
            ],
        ),
        (
            "TIME",
            "time",
            &["25:00", "12:60", "23:59:61", "24:00:01", "24:00:00.5"],
        ),
        ("TIMETZ", "time with time zone", &["z"]),
        (
            "TIMESTAMP",
            "timestamp",
            &["2026-11-26 25:00", "2026-02-30T00:00"],
        ),
        ("TIMESTAMPTZ", "timestamp with time zone", &["forever"]),
        ("INTERVAL", "interval", &["", "forever", "ago"]),
    ];
    for (sql_type, name, texts) in cases {
        for text in texts {
            assert_eq!(rejected(sql_type, text), Some(name), "{sql_type} {text:?}");
        }
    }
}
