# NativeLink

The local remote-execution executor for buck2.
NativeLink runs one process combining CAS, action cache, scheduler, and a sandboxed worker, and serves the remote-execution API on `127.0.0.1:50051`.
buck2's launcher (`tools/buck2/buck2.sh`) starts it on demand.

This directory holds the pinned installer, the two server configs, and the buck2 targets that build the worker-image pieces (busybox and a minimal rootfs) used inside the sandbox.

The worker sandbox is achieved via crun + a custom busybox root image with NativeLink bind mounted in.
The bind mount permits NativeLink to be updated without changing the rootfs, which reduces cache churn - the worker container image digest is a CAS input.

`config.json5` is used to configure NativeLink.
When BuildBuddy is in use, `config-bb.json5.template` is used to generate `.nativelink.json5` (gitignored) with the BuildBuddy API key.

## Changing the rootfs digest

`pin.bzl`'s `WORKER_LAYER_DIGEST` is the single source of truth for the worker image.
It feeds two independent things: `tools/nativelink/BUCK`'s `worker_layer` validates that `:layer_build`'s output hashes to this value, and `platforms/BUCK`'s `image_build` platform advertises it as the `container-image` remote-execution property on every `image_build`-platform action (see `platforms/defs.bzl`).

The scheduler's `container-image` property type is `exact` (`config.json5`, `config-bb.json5.template`), not `priority`: a mismatched request simply can't schedule, rather than running anyway and filing its result under the wrong image's label.
That's deliberate — it's what makes a cached action result trustworthy as "produced by this image."
A looser type like `priority` would still bust the cache on a digest bump (the property is baked into the action's hash client-side, independent of the scheduler's matching mode), but wouldn't stop a mismatched worker from actually executing the request.

The consequence: the worker construction actions (`:busybox`, `:rootfs`, `:layer_build`) are themselves `image_build`-platform actions, so they too request `container-image = <current pin>` and can only run on a worker already advertising that exact digest.
That's fine for steady state (build normally, worker and pin already agree), but it means a bare `buck2 build //tools/nativelink:layer` done right after bumping the pin can't prove much: NativeLink hasn't been restarted yet, so nothing advertises the new digest, and the construction actions fall back to local execution (`image_build` has `local_enabled = True` for exactly this reason) rather than exercising the remote worker at all.
Bare local execution also means construction never proves the new rootfs can be built using only tools present in an *already-running* (old) image — that a pin bump doesn't silently start requiring something the current worker doesn't have, i.e. that the image can always bootstrap its own successor.

To actually validate a digest bump before committing to it, run the new rootfs's construction *on the still-live old worker*, with the platform's advertised digest deliberately pointed at the old value while the content being built and checked comes from the new pin already in source.
`platforms/defs.bzl`'s `container_image` supports this via a `-c` override that takes priority over the `pin.bzl`-derived default:

```sh
# 1. On master (old pin), start a normal session — NativeLink comes up
#    advertising the old digest.
buck2 build //...

# 2. Switch the working copy to the PR (new pin), without touching the
#    running buck2 daemon or NativeLink.
jj edit <pr-branch>

# 3. Force the construction/validation actions onto the still-running old
#    worker by pinning the platform property to the OLD digest, while the
#    actions themselves build and check against whatever pin.bzl says now
#    (the new digest). A pass here proves the new rootfs is buildable and
#    hermetic using only the old image's tools.
buck2 build -c image_build.container_image=<old-digest> \
  '//tools/nativelink:layer[layer][digest]'

# 4. Only now is it safe to retire the old worker.
buck2 kill

# 5. Fresh boot picks up the new pin for both the worker's advertised
#    digest and every action's requested digest; run the full suite to
#    catch regressions under the new image.
buck2 build //...
```

If step 3 fails, the new rootfs isn't safely buildable from the current image — fix that before merging the pin bump, not after.
