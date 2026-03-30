//! Generate Rust code from proto definitions using tonic-prost-build.
//!
//! Run via Bazel: `bazel run //tools:proto_gen`
//! Output lands in `lib/rust/causes_proto/src/generated/`.
//!
//! Requires PROTOC env var pointing to the hermetic protoc binary
//! (set by the Bazel shell wrapper).
//!
//! Optional env overrides (used by the staleness check):
//!   PROTO_GEN_OUT_DIR   — output directory (default: <workspace>/lib/rust/causes_proto/src/generated)
//!   PROTO_GEN_PROTO_DIR — proto source root (default: <workspace>/proto)

use std::path::PathBuf;

fn main() {
    let workspace = workspace_root();

    let out_dir = std::env::var("PROTO_GEN_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace.join("lib/rust/causes_proto/src/generated"));

    let proto_dir = std::env::var("PROTO_GEN_PROTO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace.join("proto"));

    std::fs::create_dir_all(&out_dir).expect("creating output directory");

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(&out_dir)
        .compile_protos(
            &[proto_dir.join("causes/v1/auth_service.proto")],
            &[proto_dir],
        )
        .expect("compiling protos");

    eprintln!("generated proto code in {}", out_dir.display());
}

/// Walk up from the current directory until we find `Cargo.toml` with
/// `[workspace]` — that is the repo root.
fn workspace_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current_dir");
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate).expect("reading Cargo.toml");
            if content.contains("[workspace]") {
                return dir;
            }
        }
        if !dir.pop() {
            panic!("could not find workspace root");
        }
    }
}
