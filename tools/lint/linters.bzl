# Linter aspect definitions for the Causes monorepo.
# Each linter is defined once here and referenced from BUILD files.
# Add new language linters here as the stack grows (ADR-008).
"Linter aspects for the Causes monorepo."

load("@aspect_rules_lint//lint:buf.bzl", "lint_buf_aspect")
load("@aspect_rules_lint//lint:lint_test.bzl", "lint_test")

buf = lint_buf_aspect(
    config = Label("//proto:buf.yaml"),
)

buf_lint_test = lint_test(aspect = buf)
