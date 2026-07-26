"""Patch a `crun spec --rootless` config.json for running NativeLink.

tools/buck2/buck2.sh generates the default rootless spec with `crun spec`,
then calls this to adapt it to the proven recipe: rootless uid mapping to a
fixed in-container uid, host networking preserved (no network namespace),
and the mounts NativeLink needs (DNS, CA bundle, and its own binary/config/
work directory) without exposing anything else from the host.

argv: config.json rootfs_dir home_dir resolv_conf ca_bundle nativelink_bin
nativelink_cfg host_uid host_gid
"""

import json
import sys

# Arbitrary fixed in-container uid for the NativeLink process; only needs to
# be distinct from 0 so the rootless single-uid mapping has something to map
# to. gid maps to 0 because the busybox rootfs's non-root files are group 0.
CONTAINER_UID = 12021
CONTAINER_GID = 0


def patch(
    config_path,
    rootfs_dir,
    home_dir,
    resolv_conf,
    ca_bundle,
    nativelink_bin,
    nativelink_cfg,
    host_uid,
    host_gid,
):
    with open(config_path, encoding="utf-8") as f:
        spec = json.load(f)

    spec["root"]["path"] = rootfs_dir

    process = spec["process"]
    process["terminal"] = False
    process["user"] = {"uid": CONTAINER_UID, "gid": CONTAINER_GID}
    process["args"] = [nativelink_bin, nativelink_cfg]
    process["env"] = [
        "PATH=/bin",
        f"HOME={home_dir}",
        f"XDG_RUNTIME_DIR={home_dir}/.cache/causes-nativelink/xdg",
    ]

    linux = spec["linux"]
    linux["namespaces"] = [ns for ns in linux["namespaces"] if ns["type"] != "network"]
    linux["uidMappings"] = [{"containerID": CONTAINER_UID, "hostID": int(host_uid), "size": 1}]
    linux["gidMappings"] = [{"containerID": CONTAINER_GID, "hostID": int(host_gid), "size": 1}]

    spec["mounts"].append(
        {
            "destination": "/tmp",
            "type": "tmpfs",
            "source": "tmpfs",
            "options": ["nosuid", "strictatime", "mode=1777"],
        }
    )
    spec["mounts"].append(
        {
            "destination": "/etc/resolv.conf",
            "type": "bind",
            "source": resolv_conf,
            "options": ["bind", "ro"],
        }
    )
    if ca_bundle:
        spec["mounts"].append(
            {
                "destination": ca_bundle,
                "type": "bind",
                "source": ca_bundle,
                "options": ["bind", "ro"],
            }
        )
    cache_dir = f"{home_dir}/.cache/causes-nativelink"
    spec["mounts"].append(
        {
            "destination": cache_dir,
            "type": "bind",
            "source": cache_dir,
            "options": ["bind", "rw"],
        }
    )

    with open(config_path, "w", encoding="utf-8") as f:
        json.dump(spec, f, indent=2)


if __name__ == "__main__":
    patch(*sys.argv[1:])
