//! The `sqlty` executable of the wheel.

fn main() {
    std::process::exit(sqlty_cli::native::main());
}
