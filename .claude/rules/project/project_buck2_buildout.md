# Buck2 parallel build bring-up — in flight (2026-07-14)

Buck2 is being brought up in parallel with Bazel, aiming for eventual replacement.
It becomes load-bearing only once it builds and tests the whole tree; until then nothing routes through it.
Original motivation: a hermetic low-latency runner for the PreToolUse guard hook (warm buck2 82ms vs `bazel run` 3529ms).

## Landed

- #387: buckle launcher pinned via `.buckversion` + `.buckroot`; bundled prelude (`[external_cells] prelude = bundled`, nothing vendored); hermetic CPython 3.12.13 as `//tools:py`; `//tools:agent_action_guard`.
- #388: gating `buck2` CI job on a bazel-free checkout (`tools/buckle/install.sh`); `buck2` added to `infra/github/rulesets.tf` required checks — do not `tofu apply` until the check is green on master.
- #389 (open at time of writing): `.buckversion` 2026-06-01 → 2026-07-01.

## Gotchas that cost real time

- buck2 cannot share a checkout with Bazel: its file watcher follows the `bazel-*` convenience symlinks into the output tree and hangs, and `project.ignore` does not prevent it (buck2#465, #474).
  Dev workflow: run buck2 from a bazel-free jj workspace (`jj workspace add --revision @ ~/causes-buck2`); author and run gates in the main workspace.
- buck2 2026-06-01's RE client panics for ANY `[buck2_re_client]` address (`PathAndQuery::from_static("")`, bundled http 1.4.1) before the first connect.
  The client only shows the daemon dying ("h2 protocol error … broken pipe" on the event bus); the real panic is in `~/.buck/buckd/<repo-path>/<isolation>/buckd.stderr` — always read daemon stderr before trusting the client error.
- RE client config is read only from `.buckconfig`/`.buckconfig.local`; `-c buck2_re_client.*` command-line overrides are silently ignored.

## BuildBuddy remote cache — state

- Direct connection works on buck2 ≥ 2026-06-15: scheme-less addresses, `[buck2] digest_algorithms = SHA256`, API key via `.buckconfig.local` `http_headers` (gitignored, like `.bazelrc.user`).
- The bundled `prelude//platforms:default` has no remote cache; a custom exec platform with `remote_cache_enabled = True` is required (config exists in the scratch workspace, not yet committed).
- Cache reads verified (`GetCapabilities`, `GetActionResult` 200); uploads do NOT happen yet — executor `allow_cache_uploads = True` was insufficient for a genrule; action-level `allow_cache_upload` policy is unsolved, so no cache hit has been demonstrated.
- tonic#2185 (GOAWAY mishandling behind BuildBuddy's L7 proxy) is a real upstream bug but never reproduced here (the panic masked all earlier evidence); risk under sustained load is untested.
  A GOAWAY-shielding HTTP/2 reverse proxy (stock hyper/hyper-util, unit-tested) is shelved on branch `buck2-re-proxy-shelf` (no PR); revive only if load testing shows GOAWAY failures.
