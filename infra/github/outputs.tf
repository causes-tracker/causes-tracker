output "repository_url" {
  description = "The GitHub URL of the managed repository."
  value       = github_repository.causes.html_url
}
