# .devcontainer

Dev container configuration for VS Code and GitHub Codespaces.

The container is the canonical development environment — all tooling is installed inside it, and the Bazel cache is local to the container.
Containers are disposable; no persistent state should live here that isn't checked into the repo.

`postStartCommand` connects the container to the `causes_default` Docker network so that services started via `docker-compose` (e.g. Postgres) are reachable.
`postCreateCommand` initialises jj colocated mode and installs Claude Code globally.

## Seccomp profile

`seccomp-pivot-root.json` is docker's default seccomp profile, sourced from `github.com/moby/profiles` tag `seccomp/v0.1.0` (commit `c936cc7b4074219137bc0bee45670f5e4618d462`), with one syscall added: `pivot_root`, in the existing `CAP_SYS_ADMIN`-gated group alongside `mount`/`umount2`/`unshare`.
Docker's default profile denies `pivot_root` outright, which blocks `crun` (and any tool needing a real `pivot_root`-based rootfs switch) even with `--cap-add=SYS_ADMIN` granted.
`devcontainer.json` passes this file via `--security-opt seccomp=...`, narrower than `seccomp=unconfined`.
