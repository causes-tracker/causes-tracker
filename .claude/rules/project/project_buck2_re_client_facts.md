# buck2 RE client / BuildBuddy facts

`[buck2_re_client]` config is read only from `.buckconfig`/`.buckconfig.local`; `-c buck2_re_client.*` command-line overrides are silently ignored ("No engine address").

Working BuildBuddy config (verified 2026-07-14):
scheme-less addresses (`remote.buildbuddy.io` — `grpcs://`/`https://` are rejected), `[buck2] digest_algorithms = SHA256`, API key via `.buckconfig.local` `http_headers = x-buildbuddy-api-key:…` (gitignored, like `.bazelrc.user`), and a custom exec platform with `remote_cache_enabled = True` (the bundled `prelude//platforms:default` has no cache).

Cache uploads from locally-executed actions are gated per action, not just per executor: OSS-prelude genrules hard-code `allow_cache_uploads = False` (`prelude/genrule.bzl` cache mode) and never upload; rules exposing the `allow_cache_upload` attr (rust, cxx) upload when both the attr and the executor's `allow_cache_uploads = True` are set.
The full round-trip (upload → `buck2 clean` → rebuild with 100% action-cache hits) is verified against BuildBuddy using the test-only `BUCK2_TEST_FORCE_CACHE_UPLOAD=true` env, which bypasses the per-action gate.

tonic#2185 (GOAWAY mishandling behind BuildBuddy's L7 proxy) never reproduced here and is untested under sustained load; a GOAWAY-shielding hyper/hyper-util reverse proxy is shelved on branch `buck2-re-proxy-shelf` (no PR) — revive only if load testing shows GOAWAY failures.
