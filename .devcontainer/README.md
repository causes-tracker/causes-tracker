# .devcontainer

Dev container configuration for VS Code and GitHub Codespaces.

The container is the canonical development environment — all tooling is installed inside it, and the Bazel cache is local to the container.
Containers are disposable; no persistent state should live here that isn't checked into the repo.

`postStartCommand` connects the container to the `causes_default` Docker network so that services started via `docker-compose` (e.g. Postgres) are reachable.
`postCreateCommand` initialises jj colocated mode and installs Claude Code globally.
