# Design against requirements first; ignore current config

When designing a system, ignore the current configuration and deployment environment entirely.
Enumerate the requirements, find a candidate design that satisfies all of them, and only then map it onto how it gets deployed.

A fact about the present environment (an AppArmor sysctl on the CI runner, the current value of a config flag, what the devcontainer can or cannot do) is not a requirement.
Treating one as a requirement invents a constraint the design must "solve for" and distorts the whole space around an arbitrary starting point.

**Why:** many rounds were wasted routing a NativeLink isolation design around the CI runner's unprivileged-userns block — a deployment artifact — as if it were a design requirement.
The block evaporates once a root worker in a container is on the table, which only became visible after dropping the environment as a fixed input.

**How to apply:** design the candidate that meets the requirements, then ask where it runs.
If a deployment surface can't host the chosen design, that is a deployment problem to solve or a requirement to add explicitly — not a reason to have pre-constrained the design.
