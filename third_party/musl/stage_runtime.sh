#!/bin/sh
# Stage the shared libraries, ICU data, and requested bins from Alpine apk
# packages into one flat runtime directory.
# An apk is a concatenation of gzip tars: gzip -dc decompresses every segment
# and busybox tar reads the concatenated payload (the inner end-of-archive
# markers are stripped); decompressing to a file first keeps each step's
# failure fatal under set -e, with no pipeline to mask it.
# Usage: stage_runtime.sh <out> <bin>... -- <apk>...
set -eu

out="$1"
shift

bins=""
while [ "$1" != "--" ]; do
	bins="$bins $1"
	shift
done
shift

d="$(mktemp -d "$out.XXXXXX")"
mkdir -p "$out" "$out/bin"

# Flatten to basenames: a same-directory soname symlink stays a symlink, a
# cross-directory symlink is dereferenced (its relative target would break),
# and an already-staged basename is accepted only with identical content
# (usr/lib ships compat symlinks to lib/) - a real collision fails the build.
stage() {
	dst="$out/$(basename "$1")"
	if [ -e "$dst" ] || [ -L "$dst" ]; then
		cmp -s "$1" "$dst" || {
			echo "error: conflicting basename $(basename "$1")" >&2
			exit 1
		}
		return 0
	fi
	case "$(readlink "$1" || true)" in
	"") cp "$1" "$dst" ;;
	*/*) cp -L "$1" "$dst" ;;
	*) cp -a "$1" "$dst" ;;
	esac
}

i=0
for pkg in "$@"; do
	pd="$d/$i"
	i=$((i + 1))
	mkdir "$pd"
	gzip -dc "$pkg" >"$pd/payload.tar"
	tar -xf "$pd/payload.tar" -C "$pd"
	# Every package must contribute at least one file, so a pin whose payload
	# the globs miss is loud instead of a silently smaller runtime.
	n=0
	for f in "$pd"/lib/*.so* "$pd"/usr/lib/*.so* "$pd"/usr/share/icu/*/*.dat; do
		if [ -e "$f" ] || [ -L "$f" ]; then
			stage "$f"
			n=$((n + 1))
		fi
	done
	for b in $bins; do
		if [ -e "$pd/$b" ]; then
			dst="$out/bin/$(basename "$b")"
			if [ -e "$dst" ]; then
				cmp -s "$pd/$b" "$dst" || {
					echo "error: conflicting bin $(basename "$b")" >&2
					exit 1
				}
			else
				cp "$pd/$b" "$dst"
				chmod 0755 "$dst"
			fi
			n=$((n + 1))
		fi
	done
	if [ "$n" -eq 0 ]; then
		echo "error: $pkg staged nothing" >&2
		exit 1
	fi
done

for b in $bins; do
	if [ ! -e "$out/bin/$(basename "$b")" ]; then
		echo "error: no package provided $b" >&2
		exit 1
	fi
done

rm -rf "$d"
