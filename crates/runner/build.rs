//! Compiles `jush-start.exe` (`start/main.rs`) for a Windows target, which the
//! runner carries and writes into its installation
//! (`crates/runner/src/autostart.rs`).
//!
//! It is compiled by the `rustc`, for the target, with the linker and the
//! flags Cargo compiles the runner with, so a cross-compilation such as
//! cargo-xwin's builds it as it builds the runner. A crate of its own would
//! make a second executable of every release; this way a release still ships
//! one file for each target.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=start/main.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("jush-start.exe");
    let mut rustc = Command::new(env::var_os("RUSTC").expect("Cargo sets RUSTC"));
    rustc
        .args(["--edition", "2024", "--crate-type", "bin", "--crate-name", "jush_start"])
        .args(["-C", "opt-level=s", "-C", "panic=abort", "-C", "strip=symbols", "-C", "target-feature=+crt-static"])
        // The same bytes from every build, so an installation keeps its copy
        // across releases: no link time, and no debug record, whose PDB
        // names the build's temporary files.
        .args(["-C", "debuginfo=0", "-C", "link-arg=/DEBUG:NONE", "-C", "link-arg=/Brepro"])
        .arg("--target")
        .arg(env::var("TARGET").expect("Cargo sets TARGET"))
        .arg("-o")
        .arg(&output)
        .arg("start/main.rs");
    if let Some(linker) = env::var_os("RUSTC_LINKER") {
        let mut flag = std::ffi::OsString::from("linker=");
        flag.push(linker);
        rustc.arg("-C").arg(flag);
    }
    if let Ok(flags) = env::var("CARGO_ENCODED_RUSTFLAGS") {
        rustc.args(flags.split('\x1f').filter(|flag| !flag.is_empty()));
    }
    let status = rustc.status().expect("rustc starts");
    assert!(status.success(), "rustc could not compile start/main.rs: {status}");
}
