#!/usr/bin/env bash
# buck2 launcher: bootstrap the worker-layer pin and NativeLink the first
# time a daemon starts this session, then hand over to the pinned binary.
# Config selection: the gitignored `.nativelink.json5` at the repo root
# (the BuildBuddy overlay, see tools/nativelink/config-bb.json5.template)
# when present, else the committed local-only tools/nativelink/config.json5.
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
	buck2-bin build --local-only //tools/nativelink:layer ||
		bootstrap_rc=$?
	rm -f "$root/.buckconfig.prelaunch"
	# The daemon that ran the bootstrap build is still bound to the
	# bootstrap config (RE config only binds at daemon startup); kill it
	# so the next command starts fresh under the committed config.
	buck2-bin kill >/dev/null 2>&1 || true
	if [[ "$bootstrap_rc" -ne 0 ]]; then
		echo "error: local-only build of //tools/nativelink:layer" \
			"failed; refusing to start NativeLink over unverified bytes" >&2
		exit 1
	fi

	cfg="$root/.nativelink.json5"
	[[ -f "$cfg" ]] || cfg="$root/tools/nativelink/config.json5"
	mkdir -p "$HOME/.cache/causes-nativelink"
	(
		flock -n 9 || exit 0
		nohup nativelink "$cfg" \
			>>"$HOME/.cache/causes-nativelink/nativelink.log" 2>&1 &
	) 9>"$HOME/.cache/causes-nativelink/launch.lock"
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
