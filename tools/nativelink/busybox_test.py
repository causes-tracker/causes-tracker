"""Assert the pulled artifact is a working busybox multi-call binary.

argv[1] is the busybox binary, argv[2] the output to touch on success.
"""

import subprocess
import sys

busybox = sys.argv[1]

applets = set(
    subprocess.run([busybox, "--list"], capture_output=True, text=True, check=True).stdout.split()
)
assert {"sh", "tar", "ls", "ln"} <= applets, applets

echoed = subprocess.run(
    [busybox, "echo", "ok"], capture_output=True, text=True, check=True
).stdout.strip()
assert echoed == "ok", echoed

open(sys.argv[2], "w", encoding="utf-8").write("ok")
