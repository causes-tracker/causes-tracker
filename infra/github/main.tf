terraform {
  required_providers {
    github = {
      source  = "integrations/github"
      version = "~> 6.0"
    }
  }
}

# Authenticates via the GITHUB_TOKEN environment variable.
provider "github" {
  owner = "causes-tracker"
}

import {
  to = github_repository.causes
  id = "causes-tracker"
}

# Repository settings — captures the hand-crafted configuration.
resource "github_repository" "causes" {
  name        = "causes-tracker"
  description = "Design docs for the Causes tracker"
  visibility  = "public"

  has_issues   = true
  has_wiki     = true
  has_projects = true

  delete_branch_on_merge = true

  allow_merge_commit   = true
  allow_squash_merge   = false
  allow_rebase_merge   = false
  allow_auto_merge     = true
  merge_commit_title   = "PR_TITLE"
  merge_commit_message = "BLANK"

  vulnerability_alerts = true

  pages {
    build_type = "legacy"

    source {
      branch = "gh-pages"
      path   = "/"
    }
  }
}
