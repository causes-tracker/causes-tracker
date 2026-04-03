import {
  to = github_repository_ruleset.master
  id = "causes-tracker:13980976"
}

# Ruleset: protect the master branch.
# Requires merge queue and passing CI before merging to master.
# Mirrors the hand-crafted "Protect master" ruleset (ID 13980976).
resource "github_repository_ruleset" "master" {
  name        = "Protect master"
  repository  = github_repository.causes.name
  target      = "branch"
  enforcement = "active"

  conditions {
    ref_name {
      include = ["refs/heads/master"]
      exclude = []
    }
  }

  rules {
    deletion         = true
    non_fast_forward = true

    required_status_checks {
      strict_required_status_checks_policy = true

      required_check {
        context = "build"
      }
    }

    merge_queue {
      merge_method                    = "MERGE"
      max_entries_to_build            = 5
      min_entries_to_merge            = 1
      max_entries_to_merge            = 5
      min_entries_to_merge_wait_minutes = 0
      grouping_strategy               = "ALLGREEN"
      check_response_timeout_minutes  = 60
    }
  }
}