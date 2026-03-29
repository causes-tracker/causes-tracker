FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

# The base image ships the deprecated gold linker, which causes warnings when
# detected by build tools.  Remove the binaries; ld already points to ld.bfd.
RUN rm -f /usr/bin/x86_64-linux-gnu-ld.gold /usr/bin/ld.gold

# Install Python 3.12 (needed to bootstrap rules_python targets locally)
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3.12 python3-pip python3-venv git-delta \
    && rm -rf /var/lib/apt/lists/*

# Install Bazelisk as `bazel` — reads .bazelversion to pick the right Bazel version.
# Pin the Bazelisk version; update when a new release is available.
ARG BAZELISK_VERSION=1.28.1
RUN curl -fsSL \
      "https://github.com/bazelbuild/bazelisk/releases/download/v${BAZELISK_VERSION}/bazelisk-linux-amd64" \
      -o /usr/local/bin/bazel \
    && chmod +x /usr/local/bin/bazel

# Install Buildifier — formatter and linter for Bazel BUILD files.
ARG BUILDIFIER_VERSION=8.5.1
RUN curl -fsSL \
      "https://github.com/bazelbuild/buildtools/releases/download/v${BUILDIFIER_VERSION}/buildifier-linux-amd64" \
      -o /usr/local/bin/buildifier \
    && chmod +x /usr/local/bin/buildifier

# Install Jujutsu (jj) — version control layered over Git.
ARG JJ_VERSION=0.39.0
RUN curl -fsSL \
      "https://github.com/jj-vcs/jj/releases/download/v${JJ_VERSION}/jj-v${JJ_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
      | tar xz --strip-components=1 -C /usr/local/bin ./jj
