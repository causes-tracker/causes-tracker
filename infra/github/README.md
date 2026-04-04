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

## Merging PRs

The `merge` label is an imperative command: adding it to a PR means "merge this."
A GitHub Action (`.github/workflows/auto-queue.yml`) watches for the label.

- If the PR targets master, the action adds it to the merge queue.
- If the PR targets a feature branch, nothing happens.
  The label stays on the PR until GitHub retargets it to master (after the base branch merges and is deleted), at which point the action fires and queues it.

This replaces clicking the merge button.
Never use the merge button directly — always use the `merge` label.

### Security model

Only collaborators with write access can add labels.
External contributors cannot queue their own PRs — a maintainer must review the PR and add the label.
For a solo maintainer this is fine: you can always label your own PRs, and you review external contributions before labeling them.

GitHub does not allow self-review, so PR approval cannot be used as the merge signal for a solo maintainer.

### Stacked PR workflow

1. Create stacked PRs: `A → master`, `B → branch-a`, `C → branch-b`.
2. Add the `merge` label to all of them.
3. A targets master, so the action queues it immediately.
4. When A merges, GitHub deletes `branch-a` and retargets B to master.
5. The action detects the retarget, sees the label, and queues B.
6. The cascade continues through the stack.
