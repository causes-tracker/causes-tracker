# buck2 RE client / BuildBuddy facts

`[buck2_re_client]` config is read only from `.buckconfig`/`.buckconfig.local`; `-c buck2_re_client.*` command-line overrides are silently ignored ("No engine address").

Working BuildBuddy config (verified 2026-07-14):
scheme-less addresses (`remote.buildbuddy.io` — `grpcs://`/`https://` are rejected), `[buck2] digest_algorithms = SHA256`, API key via `.buckconfig.local` `http_headers = x-buildbuddy-api-key:…` (gitignored, like `.bazelrc.user`), and a custom exec platform with `remote_cache_enabled = True` (the bundled `prelude//platforms:default` has no cache).

Cache uploads from locally-executed actions did not happen with executor `allow_cache_uploads = True` alone; action-level `allow_cache_upload` policy was still unsolved as of 2026-07-14, so no cache hit has been demonstrated.

tonic#2185 (GOAWAY mishandling behind BuildBuddy's L7 proxy) never reproduced here and is untested under sustained load; a GOAWAY-shielding hyper/hyper-util reverse proxy is shelved on branch `buck2-re-proxy-shelf` (no PR) — revive only if load testing shows GOAWAY failures.
