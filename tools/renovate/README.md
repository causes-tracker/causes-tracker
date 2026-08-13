# tools/renovate

Scripts the `renovate-sha` CI workflow runs against a Renovate bump branch to fill in what Renovate itself can't compute (renovatebot/renovate#22183).

- `update_sha.sh` — rewrite an installer script's pinned sha256 to match its pinned version's release asset.
- `update_archive_sha.sh` — recompute pinned archive sha256s (and, for buck2 archives, their required `size_bytes`) in a `BUCK` file or `MODULE.bazel` for URLs that changed vs. the base version.

Invoked directly with `bash` from CI, not through a Bazel target: a partway-applied Renovate bump has a version/sha256 mismatch, which fails a Bazel build before these scripts could run to fix it.
The `sh_test` targets here only cover the scripts themselves under `bazel test //...`.

## `functional_check` (buck2)

Runs the real Renovate image against a fixture repo of this repo's pins, as a build action.

`GH_TOKEN` comes from `.buckconfig.local`'s `[renovate] gh_token` (gitignored), not a declared attr, so it never lands in a `BUCK` file:

```sh
printf '[renovate]\n  gh_token = %s\n' "$(gh auth token)" >> .buckconfig.local
buck2 build //tools/renovate:functional_check
```
