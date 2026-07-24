#!/usr/bin/env bash
# buck2 launcher: ensure the local NativeLink executor is up, then hand
# over to the pinned buck2 binary.
# Config selection: the gitignored `.nativelink.json5` at the repo root
# (the BuildBuddy overlay, see tools/nativelink/config-bb.json5.template)
# when present, else the committed local-only tools/nativelink/config.json5.
set -euo pipefail

port_open() {
	(exec 3<>/dev/tcp/127.0.0.1/50051) 2>/dev/null
}

# Outside a repo there is nothing to execute on; hand over directly.
root="$PWD"
while [[ ! -e "$root/.buckroot" && "$root" != / ]]; do
	root="$(dirname "$root")"
done

if [[ -e "$root/.buckroot" ]] && ! port_open; then
	cfg="$root/.nativelink.json5"
	[[ -f "$cfg" ]] || cfg="$root/tools/nativelink/config.json5"
	mkdir -p "$HOME/.cache/causes-nativelink"
	(
		flock -n 9 || exit 0
		port_open && exit 0
		nohup nativelink "$cfg" \
			>>"$HOME/.cache/causes-nativelink/nativelink.log" 2>&1 &
	) 9>"$HOME/.cache/causes-nativelink/launch.lock"
	for _ in $(seq 100); do
		port_open && break
		sleep 0.1
	done
	if ! port_open; then
		echo "warning: NativeLink did not become ready on 127.0.0.1:50051;" \
			"see ~/.cache/causes-nativelink/nativelink.log" >&2
	fi
fi

exec buck2-bin "$@"
