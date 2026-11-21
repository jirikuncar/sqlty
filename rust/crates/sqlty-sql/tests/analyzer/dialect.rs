use sqlty_sql::analyzer::*;

#[test]
fn every_name_round_trips() {
    for name in SqlDialect::NAMES {
        let dialect: SqlDialect = name.parse().unwrap();
        assert_eq!(dialect.name(), name);
    }
}

#[test]
fn names_are_case_insensitive_with_aliases() {
    assert_eq!("PostgreSQL".parse(), Ok(SqlDialect::Postgres));
    assert_eq!("TSQL".parse(), Ok(SqlDialect::Other("mssql")));
}

#[test]
fn an_unknown_name_lists_the_known_ones() {
    let error = "postgress".parse::<SqlDialect>().unwrap_err();
    assert_eq!(error.name, "postgress");
    assert!(error
        .to_string()
        .starts_with("unknown dialect \"postgress\"; expected one of: postgres, mysql,"));
}
