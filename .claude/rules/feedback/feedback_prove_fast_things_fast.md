# Prove fast things with fast checks, not slow workflows

A check that takes a second is proven by a check that takes a second: an assertion inside it, or a unit test beside it, runnable locally.
It is not proven by a CI workflow step that re-runs it against a mutated checkout, and a one-file edit outside Bazel is not verified by running the whole Bazel gate set.

**Why:** PR #690 first proved a BXL completeness check with a workflow step that wrote a probe file, re-ran buck2, and grepped the output, and the Bazel gates were run over a `.bxl` file and a workflow edit that Bazel never reads (2026-10-20).

**How to apply:** before adding a `run:` step or running a gate set, name what it proves and the cheapest check that proves it.
Tests are targets in the graph, in parallel and cached; the serial step in a workflow is only what cannot be a target.
