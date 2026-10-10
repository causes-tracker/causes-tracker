# buck2 launcher

`buck2.sh` is installed as the `buck2` command (see `install.sh`).
On the first invocation of a session it bootstraps the pinned worker layer and the NativeLink worker, then hands over to the pinned `buck2-bin`.

The launcher refuses to start while `bazel-*` convenience symlinks exist in the checkout: they make the buck2 file watcher silently stop seeing edits (mechanism in `buck2.sh`, buck2#465).
