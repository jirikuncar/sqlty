//! The `sqlty` executable on the cases of `tests/cli`; `TRYCMD=overwrite` rewrites them.

#[test]
fn cli() {
    let venv = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cli/venv");
    trycmd::TestCases::new()
        .register_bin("sqlty", std::path::Path::new(env!("CARGO_BIN_EXE_sqlty")))
        .env("VIRTUAL_ENV", venv)
        .env("SQLTY_CONFIG_FILE", "")
        .insert_var("[VERSION]", env!("CARGO_PKG_VERSION"))
        .expect("a new variable")
        .case("tests/cli/*.md");
}
