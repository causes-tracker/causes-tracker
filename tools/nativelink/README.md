# NativeLink

The local remote-execution executor for buck2.
NativeLink runs one process combining CAS, action cache, scheduler, and a sandboxed worker, and serves the remote-execution API on `127.0.0.1:50051`.
buck2's launcher (`tools/buckle/buck2.sh`) starts it on demand.

This directory holds the pinned installer, the two server configs, and the buck2 targets that build the worker-image pieces (busybox and a minimal rootfs) used inside the sandbox.

The worker sandbox is achieved via crun + a custom busybox root image with NativeLink bind mounted in.
The bind mount permits NativeLink to be updated without changing the rootfs, which reduces cache churn - the worker container image digest is a CAS input.

`config.json5` is used to configure NativeLink.
When BuildBuddy is in use, `config-bb.json5.template` is used to generate `.nativelink.json5` (gitignored) with the BuildBuddy API key.
