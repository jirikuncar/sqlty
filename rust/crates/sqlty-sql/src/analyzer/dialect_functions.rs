//! MySQL and SQLite built-ins that give NULL for some non-null arguments; their result
//! types are inference rules (`rules/mysql/`, `rules/sqlite/`).

/// MySQL functions that give NULL for some non-null arguments: a string that is not a
/// valid date or time, an index out of range, or a number outside a math function's domain.
pub(super) const MYSQL_ALWAYS_NULLABLE: &[&str] = &[
    "SQRT",
    "LN",
    "LOG",
    "LOG2",
    "LOG10",
    "ACOS",
    "ASIN",
    "JSON_SEARCH",
    "STR_TO_DATE",
    "DATE",
    "TIME",
    "TIMESTAMP",
    "FROM_UNIXTIME",
    "CONVERT_TZ",
    "MAKEDATE",
    "MAKETIME",
    "LAST_DAY",
    "ELT",
    "DATE_ADD",
    "DATE_SUB",
    "TIMESTAMPADD",
    "ADDDATE",
    "SUBDATE",
    "ADDTIME",
    "SUBTIME",
    "DATABASE",
    "SCHEMA",
    "INET_ATON",
    "INET_NTOA",
    "INET6_ATON",
    "INET6_NTOA",
    "CONV",
    "FROM_BASE64",
    "GET_FORMAT",
    "AES_DECRYPT",
];

/// SQLite functions that give NULL for a value they cannot read: the date functions for
/// one that is not a date, `unhex` for one that is not hexadecimal, the math functions for
/// one outside their domain.
pub(super) const SQLITE_ALWAYS_NULLABLE: &[&str] = &[
    "SQRT",
    "LN",
    "LOG",
    "LOG2",
    "LOG10",
    "ACOS",
    "ASIN",
    "ACOSH",
    "ATANH",
    "POW",
    "POWER",
    "UNHEX",
    "DATE",
    "TIME",
    "DATETIME",
    "STRFTIME",
    "JULIANDAY",
    "UNIXEPOCH",
    "TIMEDIFF",
];
