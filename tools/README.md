# tools

Hermetic developer tooling wrapped for Bazel.
No host-installed `cargo`, `rustc`, `rustfmt`, `sqlx`, or `protoc` is needed — every tool is fetched and pinned by Bazel.

Notable:

- `proto_gen` regenerates checked-in proto bindings via `tonic-prost-build`.
  A golden-file staleness test (`proto_gen_test`) runs in CI.
- `coverage.sh` runs `bazel coverage` and enforces per-file thresholds (25% minimum for Rust sources in `services/` and `lib/rust/`).
  This is the script CI runs — not bare `bazel test`.
