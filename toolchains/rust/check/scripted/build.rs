use std::{env, fs, path::Path, process::Command};

fn main() {
    let rustc = env::var("RUSTC").expect("RUSTC is set");
    let version = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(version.status.success(), "rustc --version failed");
    let version = String::from_utf8(version.stdout).expect("version is utf-8");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set");
    // Build scripts run the C compiler from directories under OUT_DIR.
    let build_dir = Path::new(&out_dir).join("build");
    fs::create_dir(&build_dir).expect("OUT_DIR is writable");
    let cc = env::var("CC").expect("CC is set");
    let cc_version = Command::new(cc)
        .arg("--version")
        .current_dir(&build_dir)
        .output()
        .expect("cc runs");
    assert!(
        cc_version.status.success(),
        "cc --version failed: {}",
        String::from_utf8_lossy(&cc_version.stderr)
    );
    fs::write(
        Path::new(&out_dir).join("version.rs"),
        format!("pub const RUSTC_VERSION: &str = {:?};\n", version.trim()),
    )
    .expect("OUT_DIR is writable");
}
