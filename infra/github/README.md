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

## Auto-queue token (`AUTO_QUEUE_TOKEN`)

The auto-queue workflow (`.github/workflows/auto-queue.yml`) uses a fine-grained PAT stored as the repository secret `AUTO_QUEUE_TOKEN`.
A PAT is required because actions triggered by the built-in `GITHUB_TOKEN` [cannot trigger further workflow runs][gh-token-limit] — so the `merge_group` event that the merge queue needs would never fire.

[gh-token-limit]: https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication#using-the-github_token-in-a-workflow

### Creating or rotating the token

1. Go to **GitHub → Settings → Developer settings → Fine-grained personal access tokens → Generate new token**.
2. Configure:
   - **Token name:** `causes-auto-queue` (or any descriptive name)
   - **Expiration:** 90 days (or your preferred rotation cadence)
   - **Repository access:** Only select repositories → `causes-tracker/causes-tracker`
   - **Permissions:**

     | Permission | Access | Why |
     |---|---|---|
     | Contents | Read and write | Required by `gh pr merge` |
     | Pull requests | Read and write | Enable auto-merge on PRs |
     | Merge queues | Read and write | Add PRs to the merge queue |
     | Metadata | Read-only | Required by all fine-grained tokens |

3. Copy the token.
4. Go to the repository **Settings → Secrets and variables → Actions → Repository secrets**.
5. Create or update the secret named `AUTO_QUEUE_TOKEN` with the token value.
6. Verify by adding the `merge` label to any PR — the auto-queue action should succeed and the merge queue build should start.

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
