# pgvector is built into Aurora PostgreSQL and does not require
# shared_preload_libraries.
# Run this once after the cluster is reachable from inside the VPC:
#
#   CREATE EXTENSION IF NOT EXISTS vector;
#
# The cluster endpoint is private (no public access), so this cannot
# be automated from a local-exec provisioner running on the developer's machine.
