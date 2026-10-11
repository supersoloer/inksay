use std::{env, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=INKSAY_BUILD_NUMBER");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
    println!("cargo:rerun-if-changed=../.git/refs");
    let build = env::var("INKSAY_BUILD_NUMBER").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("invalid system clock")
            .as_secs()
            .to_string()
    });
    assert!(
        !build.is_empty() && build.bytes().all(|b| b.is_ascii_digit()),
        "INKSAY_BUILD_NUMBER must be numeric"
    );
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .args(args)
            .output()
            .expect("git required for build provenance");
        assert!(result.status.success(), "git provenance failed");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    println!("cargo:rustc-env=INKSAY_BUILD_NUMBER={build}");
    println!(
        "cargo:rustc-env=INKSAY_GIT_COMMIT={}",
        git(&["rev-parse", "HEAD"])
    );
    println!(
        "cargo:rustc-env=INKSAY_GIT_DIRTY={}",
        !git(&["status", "--porcelain"]).is_empty()
    );
    println!(
        "cargo:rustc-env=INKSAY_TARGET={}",
        env::var("TARGET").unwrap()
    );
    let compiler = Command::new(env::var("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("rustc version");
    println!(
        "cargo:rustc-env=INKSAY_RUST_VERSION={}",
        String::from_utf8(compiler.stdout).unwrap().trim()
    );
    tauri_build::build()
}
