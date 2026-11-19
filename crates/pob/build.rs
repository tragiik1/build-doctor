//! Compiles `lua-utf8` (a C module PoB requires) against the vendored LuaJIT
//! headers, so it can be linked statically and handed to Lua as a preload.

use std::path::PathBuf;

fn main() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let luautf8 = workspace.join("vendor/luautf8");
    let luajit_headers = workspace.join("vendor/luajit-src/luajit2/src");

    println!("cargo:rerun-if-changed={}", luautf8.display());

    cc::Build::new()
        .file(luautf8.join("lutf8lib.c"))
        .include(&luautf8)
        .include(&luajit_headers)
        .warnings(false)
        .compile("luautf8");
}
