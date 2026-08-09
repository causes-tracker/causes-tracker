#!/usr/bin/env bash
# buck2 launcher: bootstrap the worker-layer pin and NativeLink the first
# time a daemon starts this session, then hand over to the pinned binary.
# Config selection: the gitignored `.nativelink.json5` at the repo root
# (the BuildBuddy overlay, see tools/nativelink/config-bb.json5.template)
# when present, else the committed local-only tools/nativelink/config.json5.
#
# tools/nativelink/crun-bundle-config.json.template is `crun spec --rootless`'s
# output with NativeLink's overrides baked in — the omitted network namespace
# is why buck2 can still reach NativeLink on 127.0.0.1:50051.
set -euo pipefail

# Outside a repo there is nothing to execute on; hand over directly.
root="$PWD"
while [[ ! -e "$root/.buckroot" && "$root" != / ]]; do
	root="$(dirname "$root")"
done

# A live daemon means this repo is already bootstrapped for this session.
no_daemon() {
	# Match on the output, not the pipe exit (pipefail would let a failed
	# status mask the grep).
	local out
	out="$(buck2-bin status 2>&1)" || true
	grep -qx 'no buckd running' <<<"$out"
}

if [[ -e "$root/.buckroot" ]] && no_daemon; then
	# Clear any stale worker, then confirm it is gone before starting a new one.
	pkill -x nativelink 2>/dev/null || [ "$?" -eq 1 ]
	for _ in $(seq 50); do
		pgrep -x nativelink >/dev/null || break
		sleep 0.1
	done
	if pgrep -x nativelink >/dev/null; then
		echo "error: stale nativelink survived kill" >&2
		exit 1
	fi

	if [[ -f "$root/.buckconfig.local" ]]; then
		printf '[buck2_re_client]\n%s\n%s\n%s\n%s\n' \
			'  engine_address = remote.buildbuddy.io' \
			'  action_cache_address = remote.buildbuddy.io' \
			'  cas_address = remote.buildbuddy.io' \
			'  tls = true' \
			>"$root/.buckconfig.prelaunch"
	fi
	bootstrap_rc=0
	targets=(
		//tools/nativelink:busybox
		//tools/nativelink:layer
		'//tools/nativelink:layer[layer][digest]'
	)
	# Stream the build so its superconsole shows; a captured $(...) is not a
	# TTY, so buck2 hides all progress and the ~20s bootstrap looks hung.
	buck2-bin build --local-only "${targets[@]}" || bootstrap_rc=$?
	# Re-query the now-cached build for the output paths: a no-op that streams
	# nothing, so capturing it hides no progress. Sorted by target label, which
	# the sed -n below relies on.
	if [[ "$bootstrap_rc" -eq 0 ]]; then
		outputs="$(buck2-bin build --local-only --show-full-simple-output "${targets[@]}")"
	fi
	rm -f "$root/.buckconfig.prelaunch"
	# The daemon that ran the bootstrap build is still bound to the
	# bootstrap config (RE config only binds at daemon startup); kill it
	# so the next command starts fresh under the committed config.
	buck2-bin kill >/dev/null 2>&1 || true
	no_daemon || {
		echo "error: buck2 daemon alive after kill" >&2
		exit 1
	}
	if [[ "$bootstrap_rc" -ne 0 ]]; then
		# buck2's layer validation already printed the expected and actual
		# digest and whether the rootfs content or only the tar serialization
		# differs.
		buck2-bin kill >/dev/null 2>&1 || true
		echo "error: refusing to start NativeLink over unverified bytes" >&2
		exit 1
	fi
	busybox="$(sed -n '1p' <<<"$outputs")"
	layer_tar="$(sed -n '2p' <<<"$outputs")"
	digest="$(cat "$(sed -n '3p' <<<"$outputs")")"

	cfg="$root/.nativelink.json5"
	[[ -f "$cfg" ]] || cfg="$root/tools/nativelink/config.json5"
	nl_cache="$HOME/.cache/causes-nativelink"
	mkdir -p "$nl_cache/bin" "$nl_cache/xdg"
	cp -f "$(command -v nativelink)" "$nl_cache/bin/nativelink"
	# Advertised as the container-image platform property (see
	# platforms/defs.bzl) so buck2 bakes it into every image_build action's
	# digest, and bumping the pin invalidates NativeLink's cache for
	# actions that ran under the old image.
	sed -e "s|@LAYER_DIGEST@|$digest|g" "$cfg" >"$nl_cache/config.json5"

	rootfs="$nl_cache/rootfs-$digest"
	if [[ ! -d "$rootfs" ]]; then
		tmp_rootfs="$(mktemp -d "$nl_cache/rootfs-$digest.XXXXXX")"
		"$busybox" tar x -f "$layer_tar" -C "$tmp_rootfs"
		mv -T "$tmp_rootfs" "$rootfs"
	fi

	bundle="$nl_cache/crun-bundle"
	rm -rf "$bundle"
	mkdir -p "$bundle"
	sed \
		-e "s|@ROOTFS@|$rootfs|g" \
		-e "s|@HOME@|$HOME|g" \
		-e "s|@NL_CACHE@|$nl_cache|g" \
		-e "s|@HOST_UID@|$(id -u)|g" \
		-e "s|@HOST_GID@|$(id -g)|g" \
		"$root/tools/nativelink/crun-bundle-config.json.template" \
		>"$bundle/config.json"

	container="causes-nativelink-$digest"
	export XDG_RUNTIME_DIR="$nl_cache/xdg"
	crun delete -f "$container" >/dev/null 2>&1 || true
	if crun state "$container" >/dev/null 2>&1; then
		echo "error: container $container survived delete" >&2
		exit 1
	fi
	(
		flock -n 9 || exit 0
		crun run --bundle "$bundle" --detach --no-new-keyring "$container" \
			>>"$nl_cache/nativelink.log" 2>&1
	) 9>"$nl_cache/launch.lock"
	for _ in $(seq 100); do
		(exec 3<>/dev/tcp/127.0.0.1/50051) 2>/dev/null && break
		sleep 0.1
	done
	if ! (exec 3<>/dev/tcp/127.0.0.1/50051) 2>/dev/null; then
		echo "error: NativeLink did not become ready on 127.0.0.1:50051;" \
			"see ~/.cache/causes-nativelink/nativelink.log" >&2
		exit 1
	fi
fi

exec buck2-bin "$@"
