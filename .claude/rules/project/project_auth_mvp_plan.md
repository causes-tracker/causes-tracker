---
name: Auth + Projects feature status
description: Auth MVP and projects feature both complete as of 2026-04-05
type: project
---

## Authenticated gRPC MVP — complete (2026-03-31)

Sessions, login RPCs, auth interceptor, CLI, TLS, auto-provisioning.

## Authorization layer — complete (2026-04-02)

Role enum, DB queries, authorization helpers, GrantRole RPC + CLI.
Restricted sessions suppress instance-admin. `--admin` flag for escalation.

## Projects feature — complete (2026-04-05)

Create/rename/delete projects with authorization enforcement.
Private project visibility (ProjectAccess enum: Visible/AccessDenied/NotFound).
Streaming list_projects with batched visibility checks.
Proto uses project names (slugs), not UUIDs.
CLI with testable output (Write trait) and --visibility flag.

**How to apply:** All done. Start fresh plans for new features.
