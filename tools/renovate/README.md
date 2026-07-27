# tools/renovate

Scripts the `renovate-sha` CI workflow runs against a Renovate bump branch to fill in what Renovate itself can't compute (renovatebot/renovate#22183).

- `update_sha.sh` — rewrite an installer script's pinned sha256 to match its pinned version's release asset.
- `update_archive_sha.sh` — recompute pinned archive sha256s in a `BUCK` file or `MODULE.bazel` for URLs that changed vs. the base version.

Invoked directly with `bash` from CI, not through a Bazel target: a partway-applied Renovate bump has a version/sha256 mismatch, which fails a Bazel build before these scripts could run to fix it.
The `sh_test` targets here only cover the scripts themselves under `bazel test //...`.
