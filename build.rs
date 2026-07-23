use std::{env, fs::File, io::Write, path::PathBuf, process::Command};

const VERSION_OVERRIDE: &str = "BONANZA_BRIDGE_FW_VERSION";
const VERSION_MAX_LENGTH: usize = 63;

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn default_firmware_version() -> String {
    let package_version = env::var("CARGO_PKG_VERSION").unwrap();
    let Some(revision) = git_output(&["rev-parse", "--short=7", "HEAD"]) else {
        return package_version;
    };

    let dirty = git_output(&["status", "--porcelain", "--untracked-files=normal"]).is_some();
    format!("{package_version}+g{revision}{}", if dirty { ".dirty" } else { "" })
}

fn watch_git_state() {
    for path in ["HEAD", "index"] {
        if let Some(path) = git_output(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    if let Some(reference) = git_output(&["symbolic-ref", "HEAD"]) {
        if let Some(path) = git_output(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn validate_firmware_version(version: &str) {
    assert!(!version.is_empty(), "firmware version must not be empty");
    assert!(version.len() <= VERSION_MAX_LENGTH, "firmware version must be at most {VERSION_MAX_LENGTH} bytes");
    assert!(version.bytes().all(|byte| (0x20..=0x7e).contains(&byte)), "firmware version must contain printable ASCII only");
}

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x")).unwrap().write_all(include_bytes!("memory.x")).unwrap();

    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    println!("cargo:rerun-if-env-changed={VERSION_OVERRIDE}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    watch_git_state();

    let firmware_version = env::var(VERSION_OVERRIDE).unwrap_or_else(|_| default_firmware_version());
    validate_firmware_version(&firmware_version);
    println!("cargo:rustc-env=BRIDGE_FIRMWARE_VERSION={firmware_version}");
}
