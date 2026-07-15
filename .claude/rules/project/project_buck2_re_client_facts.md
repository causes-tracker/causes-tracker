# buck2 RE client / BuildBuddy facts

`[buck2_re_client]` config is read only from `.buckconfig`/`.buckconfig.local`; `-c buck2_re_client.*` command-line overrides are silently ignored ("No engine address").

Working BuildBuddy config (verified 2026-07-14):
scheme-less addresses (`remote.buildbuddy.io` — `grpcs://`/`https://` are rejected), `[buck2] digest_algorithms = SHA256`, API key via `.buckconfig.local` `http_headers = x-buildbuddy-api-key:…` (gitignored, like `.bazelrc.user`), and a custom exec platform with `remote_cache_enabled = True` (the bundled `prelude//platforms:default` has no cache).

Uploads from locally-executed actions require per-action `allow_cache_upload = True` ANDed with the executor's `allow_cache_uploads`; the OSS prelude hard-codes uploads off for `genrule` and never sets the flag on `http_archive`'s unpack action, so cache-participating targets use the custom rules in `tools/defs.bzl` (`cached_check`, `cached_http_archive`).
The round-trip (upload, then 100% hits after `buck2 clean` + `buck2 kill`) is asserted on every CI run (#392, verified 2026-07-15).

tonic#2185 (GOAWAY mishandling behind BuildBuddy's L7 proxy) never reproduced here and is untested under sustained load; a GOAWAY-shielding hyper/hyper-util reverse proxy is shelved on branch `buck2-re-proxy-shelf` (no PR) — revive only if load testing shows GOAWAY failures.
