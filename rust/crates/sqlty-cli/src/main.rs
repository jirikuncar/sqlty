//! A standalone `sqlty` executable over [`sqlty_cli::native`].

fn main() {
    std::process::exit(sqlty_cli::native::main());
}
