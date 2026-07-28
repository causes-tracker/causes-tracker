#!/usr/bin/env bash
# Runs the real Renovate image (via crun) against a fixture repo of this
# repo's pins and asserts its managers extract them.
# A buck2 build action, not a test, so it's cacheable: re-runs only when a
# declared input (this script, renovate.json, the pinned image digest,
# crane/nativelink) changes — whether upstream has published something new
# is not tracked, by design (that's what the pin freezes).
# GH_TOKEN comes from the environment, never from a declared input — see
# functional_check.bzl for how it's threaded through and why this can never
# reach a remote executor.
#
# NOT YET HERMETIC: crun, git, and jq are all called off PATH, none staged
# as declared buck2 inputs.
#
# Args: crane crun_install nativelink_install nativelink_buck crane_buck \
#       renovate_json renovate_image out_stamp
set -euo pipefail

crane="$1"
crun_install="$2"
nativelink_install="$3"
nativelink_buck="$4"
crane_buck="$5"
renovate_json="$6"
renovate_image="$7"
out="$8"

token="${GH_TOKEN:-}"
if [[ -z "$token" ]]; then
	echo "ERROR: GH_TOKEN is not set. Configure it in .buckconfig.local:" \
		"[renovate]\\n  gh_token = <token>" >&2
	exit 1
fi

work="$(mktemp -d)"
xdg="$(mktemp -d)"
trap 'rm -rf "$work" "$xdg"' EXIT

bindir="$work/bin"
mkdir -p "$bindir"
install -m 0755 "$crane" "$bindir/crane"
export PATH="$bindir:$PATH"

# ---- fixture repo: the real renovate.json, backdated pins ----
repo="$work/repo"
mkdir -p "$repo/tools/nativelink" "$repo/tools/crun" \
	"$repo/third_party/crane" "$repo/third_party/python" "$repo/.devcontainer"
cp "$renovate_json" "$repo/renovate.json"
sed 's/^NATIVELINK_VERSION=.*/NATIVELINK_VERSION=1.5.0/' \
	"$nativelink_install" >"$repo/tools/nativelink/install.sh"
sed 's/^CRUN_VERSION=.*/CRUN_VERSION=1.20/' \
	"$crun_install" >"$repo/tools/crun/install.sh"
sed -E 's/busybox:[0-9.]+-musl@sha256:[0-9a-f]+/busybox:1.36.0-musl@sha256:0000000000000000000000000000000000000000000000000000000000000000/' \
	"$nativelink_buck" >"$repo/tools/nativelink/BUCK"
sed 's|download/v[0-9.]*/|download/v0.19.0/|' \
	"$crane_buck" >"$repo/third_party/crane/BUCK"
# The multi-axis pin: cpython version and python-build-standalone date share
# one URL; the grouped update must move both axes together.
cat >"$repo/third_party/python/BUCK" <<'EOF'
cached_http_archive(
    name = "cpython",
    sha256 = "0000000000000000000000000000000000000000000000000000000000000000",
    strip_components = 1,
    urls = ["https://github.com/astral-sh/python-build-standalone/releases/download/20240814/cpython-3.12.0+20240814-x86_64-unknown-linux-gnu-install_only.tar.gz"],
    visibility = ["PUBLIC"],
)
EOF
# A MODULE.bazel pin absent from every BUCK file, so the MODULE.bazel
# manager pattern is load-bearing for its assertion.
cat >"$repo/MODULE.bazel" <<'EOF'
http_archive(
    name = "uv_linux_amd64",
    sha256 = "8c88519b0ef0af9801fcdee419bbb12116bd9e6b18e162ae093c932d8b264050",
    url = "https://github.com/astral-sh/uv/releases/download/0.11.0/uv-x86_64-unknown-linux-gnu.tar.gz",
)
EOF
# devcontainer feature pins: version and digest backdated together so the
# customManagers regex over the lockfile must move both in one update.
cat >"$repo/.devcontainer/devcontainer-lock.json" <<'EOF'
{
  "features": {
    "ghcr.io/devcontainers/features/docker-outside-of-docker:1": {
      "version": "1.9.0",
      "resolved": "ghcr.io/devcontainers/features/docker-outside-of-docker@sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "integrity": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    },
    "ghcr.io/devcontainers/features/github-cli:1": {
      "version": "1.0.0",
      "resolved": "ghcr.io/devcontainers/features/github-cli@sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "integrity": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    },
    "ghcr.io/devcontainers/features/node:1": {
      "version": "1.6.0",
      "resolved": "ghcr.io/devcontainers/features/node@sha256:0000000000000000000000000000000000000000000000000000000000000000",
      "integrity": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    }
  }
}
EOF
git -C "$repo" -c init.defaultBranch=master init -q
git -C "$repo" -c user.email=test@test -c user.name=test add -A
git -C "$repo" -c user.email=test@test -c user.name=test \
	commit -q -m "fixture: renovate pins"

# ---- OCI bundle: export the real (digest-pinned) renovate image, no Docker daemon ----
bundle="$work/bundle"
mkdir -p "$bundle/rootfs"
crane export --platform linux/amd64 "$renovate_image" - |
	tar -x -C "$bundle/rootfs"

(cd "$bundle" && crun spec --rootless)

uid="$(id -u)"
gid="$(id -g)"
config="$bundle/config.json"
jq \
	--argjson uid "$uid" \
	--argjson gid "$gid" \
	--arg repo "$repo" \
	--arg token "$token" \
	'.process.terminal = false
	 | .process.user.uid = 12021
	 | .process.user.gid = 0
	 | .process.args = ["renovate"]
	 | .process.cwd = "/repo"
	 | .process.env += [
	     "RENOVATE_PLATFORM=local",
	     "LOG_LEVEL=debug",
	     "GITHUB_COM_TOKEN=" + $token,
	     "RENOVATE_TOKEN=" + $token
	   ]
	 | .linux.uidMappings = [{"containerID": 12021, "hostID": $uid, "size": 1}]
	 | .linux.gidMappings = [{"containerID": 0, "hostID": $gid, "size": 1}]
	 | .linux.namespaces = [.linux.namespaces[] | select(.type != "network")]
	 | .mounts += [
	     {"destination": "/repo", "type": "bind", "source": $repo,
	      "options": ["rbind", "rw"]},
	     {"destination": "/etc/resolv.conf", "type": "bind",
	      "source": "/etc/resolv.conf", "options": ["rbind", "ro"]},
	     {"destination": "/tmp", "type": "tmpfs", "source": "tmpfs",
	      "options": ["nosuid", "nodev", "mode=1777"]}
	   ]' \
	"$config" >"$config.new"
mv "$config.new" "$config"

log="$work/renovate.log"
XDG_RUNTIME_DIR="$xdg" crun run --no-new-keyring -b "$bundle" \
	"renovate-functional-check-$$" >"$log" 2>&1
rc=$?

fail=0
if [[ "$rc" -ne 0 ]]; then
	echo "FAIL: renovate exited $rc"
	fail=1
fi

# A dep's update window runs from its depName line to the next depName line,
# so a neighbouring dep's newValue cannot satisfy the wrong assertion.
update_proposed() {
	awk -v dep="\"depName\": \"$1\"" '
		/"depName": "/ { cur = (index($0, dep) > 0) }
		cur && /"newValue"/ { found = 1 }
		END { exit !found }
	' "$log"
}

# Every backdated pin must yield an update decision, not just extract.
for dep in TraceMachina/nativelink containers/crun astral-sh/uv \
	google/go-containerregistry busybox \
	python/cpython astral-sh/python-build-standalone \
	ghcr.io/devcontainers/features/docker-outside-of-docker \
	ghcr.io/devcontainers/features/github-cli \
	ghcr.io/devcontainers/features/node; do
	if grep -qF "\"depName\": \"${dep}\"" "$log"; then
		echo "PASS: extracted depName ${dep}"
	else
		echo "FAIL: depName ${dep} not found in renovate output"
		fail=1
	fi
	if update_proposed "$dep"; then
		echo "PASS: update proposed for ${dep}"
	else
		echo "FAIL: no update proposed for ${dep}"
		fail=1
	fi
done

# python/cpython tags alpha/beta/rc candidates upstream (e.g. v3.15.0b4);
# any proposed newValue must be a plain X.Y.Z release, never a pre-release.
cpython_new_value() {
	awk -v dep="\"depName\": \"python/cpython\"" '
		/"depName": "/ { cur = (index($0, dep) > 0) }
		cur && /"newValue"/ { print; exit }
	' "$log" | grep -oE '"[0-9]+\.[0-9]+\.[0-9]+[a-zA-Z0-9]*"' | tr -d '"'
}
new_value="$(cpython_new_value)"
if [[ -z "$new_value" ]]; then
	echo "FAIL: no newValue found for python/cpython to check"
	fail=1
elif [[ "$new_value" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
	echo "PASS: python/cpython newValue ${new_value} is a stable release"
else
	echo "FAIL: python/cpython newValue ${new_value} is a pre-release"
	fail=1
fi

# The multi-axis pair moves together: one grouped branch carries both the
# cpython version and the python-build-standalone date.
if grep -qF '"branchName": "renovate/cpython"' "$log"; then
	echo "PASS: cpython and python-build-standalone group into renovate/cpython"
else
	echo "FAIL: no grouped renovate/cpython branch in renovate output"
	fail=1
fi

if grep -q "external-host-error" "$log"; then
	echo "FAIL: renovate reported external-host-error"
	grep -n "external-host-error" "$log"
	fail=1
else
	echo "PASS: zero external-host-error"
fi

if [[ "$fail" -ne 0 ]]; then
	echo "---- renovate log tail ----" >&2
	tail -200 "$log" >&2
	exit 1
fi

echo "ok" >"$out"
