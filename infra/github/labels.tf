# Imperative label: adding this to a PR signals "merge this".
# The auto-queue workflow (.github/workflows/auto-queue.yml) acts on it:
# if the PR targets master, it enters the merge queue; otherwise, nothing
# happens until GitHub retargets the PR to master after its base merges.
resource "github_issue_label" "merge" {
  repository  = github_repository.causes.name
  name        = "merge"
  color       = "0E8A16"
  description = "Queue this PR for merging when it targets master."
}
