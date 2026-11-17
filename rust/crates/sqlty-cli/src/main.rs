//! A standalone `sqlty` executable over [`sqlty_cli::native`].

#[cfg(target_env = "musl")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    std::process::exit(sqlty_cli::native::main());
}
