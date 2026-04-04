# infra/github

Manages the causes-tracker GitHub repository configuration: repository settings, branch rulesets, and merge queue.

All commands below use the hermetic OpenTofu wrapper:
`bazel run //infra:tofu -- infra/github <args>`.

## Prerequisites

Create a fine-grained personal access token scoped to the `causes-tracker/causes-tracker` repository with these permissions:

| Permission | Access | Why |
|---|---|---|
| Administration | Read and write | Manage rulesets and repository settings |
| Metadata | Read-only | Required by all fine-grained tokens |

Export it before running any commands:

```sh
export GITHUB_TOKEN="github_pat_..."
```

## First-time setup

### 1. Initialise the module

```sh
bazel run //infra:tofu -- infra/github init
```

### 2. Plan and verify

The repository and its master ruleset were created by hand.
`import` blocks in `main.tf` and `rulesets.tf` adopt them automatically on the first apply.

```sh
bazel run //infra:tofu -- infra/github plan
```

Review the plan.
Adjust `.tf` files if there is drift between the hand-crafted settings and the declared configuration.
Once the plan shows no changes (or only intended ones):

```sh
bazel run //infra:tofu -- infra/github apply
```

## Day-to-day usage

```sh
bazel run //infra:tofu -- infra/github plan    # preview changes
bazel run //infra:tofu -- infra/github apply   # apply changes
```
