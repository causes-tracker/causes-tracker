#!/usr/bin/env bash
# buck2 launcher: bootstrap the worker-image pin and NativeLink the first
# time a daemon starts this session, then hand over to the pinned binary.
# Config selection: the gitignored `.nativelink.json5` at the repo root
# (the BuildBuddy overlay, see tools/nativelink/config-bb.json5.template)
# when present, else the committed local-only tools/nativelink/config.json5.
#
# NativeLink itself runs inside a crun container built from the pinned
# worker image (tools/nativelink:worker_image), not as a bare host process,
# so actions it executes see the busybox-musl worker rootfs instead of this
# checkout. Host networking is preserved (no network namespace) so buck2 can
# still reach it on 127.0.0.1:50051.
set -euo pipefail

# Outside a repo there is nothing to execute on; hand over directly.
root="$PWD"
while [[ ! -e "$root/.buckroot" && "$root" != / ]]; do
	root="$(dirname "$root")"
done

# A live daemon means this repo is already bootstrapped for this session.
no_daemon() {
	buck2-bin status 2>&1 | grep -qx 'no buckd running'
}

if [[ -e "$root/.buckroot" ]] && no_daemon; then
	pkill -x nativelink 2>/dev/null || true

	if [[ -f "$root/.buckconfig.local" ]]; then
		printf '[buck2_re_client]\n%s\n%s\n%s\n%s\n' \
			'  engine_address = remote.buildbuddy.io' \
			'  action_cache_address = remote.buildbuddy.io' \
			'  cas_address = remote.buildbuddy.io' \
			'  tls = true' \
			>"$root/.buckconfig.prelaunch"
	fi
	bootstrap_rc=0
	worker_tar="$(buck2-bin build --local-only --show-full-simple-output \
		//tools/nativelink:worker_image)" || bootstrap_rc=$?
	rm -f "$root/.buckconfig.prelaunch"
	# The daemon that ran the bootstrap build is still bound to the
	# bootstrap config (RE config only binds at daemon startup); kill it
	# so the next command starts fresh under the committed config.
	buck2-bin kill >/dev/null 2>&1 || true
	if [[ "$bootstrap_rc" -ne 0 ]]; then
		echo "error: local-only build of //tools/nativelink:worker_image" \
			"failed; refusing to start NativeLink over unverified bytes" >&2
		exit 1
	fi

	cfg="$root/.nativelink.json5"
	[[ -f "$cfg" ]] || cfg="$root/tools/nativelink/config.json5"
	nl_cache="$HOME/.cache/causes-nativelink"
	mkdir -p "$nl_cache/bin" "$nl_cache/xdg"
	cp -f "$(command -v nativelink)" "$nl_cache/bin/nativelink"
	cp -f "$cfg" "$nl_cache/config.json5"

	# The rootfs extraction is content-addressed by the image tar's digest
	# (hashed here rather than read from the digest sub-target, since the
	# tar is already local from the bootstrap build above) so repeated
	# launches with an unchanged worker image reuse it instead of paying
	# the extraction cost on every cold start.
	digest="$(sha256sum "$worker_tar" | cut -d' ' -f1)"
	rootfs="$nl_cache/rootfs-$digest"
	if [[ ! -d "$rootfs" ]]; then
		echo "buck2.sh: extracting worker rootfs (digest $digest)" >&2
		tmp_rootfs="$(mktemp -d "$nl_cache/rootfs-$digest.XXXXXX")"
		python3 "$root/tools/nativelink/extract_rootfs.py" \
			"$worker_tar" "$tmp_rootfs"
		mv -T "$tmp_rootfs" "$rootfs"
	fi

	bundle="$nl_cache/crun-bundle"
	rm -rf "$bundle"
	mkdir -p "$bundle"
	crun spec --rootless --bundle "$bundle"
	ca_bundle=""
	[[ -f /etc/ssl/certs/ca-certificates.crt ]] &&
		ca_bundle=/etc/ssl/certs/ca-certificates.crt
	python3 "$root/tools/nativelink/patch_crun_spec.py" \
		"$bundle/config.json" "$rootfs" "$HOME" /etc/resolv.conf \
		"$ca_bundle" "$nl_cache/bin/nativelink" "$nl_cache/config.json5" \
		"$(id -u)" "$(id -g)"

	container="causes-nativelink-$digest"
	export XDG_RUNTIME_DIR="$nl_cache/xdg"
	crun delete -f "$container" >/dev/null 2>&1 || true
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
		echo "warning: NativeLink did not become ready on 127.0.0.1:50051;" \
			"see ~/.cache/causes-nativelink/nativelink.log" >&2
	fi
fi

exec buck2-bin "$@"
