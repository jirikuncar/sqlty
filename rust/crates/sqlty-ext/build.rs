//! Lets the tests, which embed Python, find libpython.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=PYO3_BUILD_EXTENSION_MODULE");
    if env::var_os("PYO3_BUILD_EXTENSION_MODULE").is_none() {
        if let Some(lib_dir) = pyo3_build_config::get().lib_dir() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
        }
    }
}
