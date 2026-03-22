# Design Choices

This document captures the technical design decisions for Causes, freeing [Manifesto.md](Manifesto.md) to focus on conceptual analysis.
Implementers should read both.
For the rationale behind each decision, see [Decisions.md](Decisions.md).

## Data distribution: centralised / distributed / replicated

Distributed and replicated.
Some data sets (e.g. crash reports) may be too large to replicate everywhere; others (current project plans) are highly useful offline.
The architecture supports both.
The private-company use case — full local mirror of a public project's bugs, with selective upstream publishing — is a good test of the design.

See [ADR-001](Decisions.md#adr-001-distributed-and-replicated-data-model).

## No hard dependencies on hosted-only web services

We can use anything for which a completely offline, self-hostable implementation exists.
Anything else would prevent running private sites.

See [ADR-003](Decisions.md#adr-003-no-hard-dependencies-on-proprietary-or-hosted-only-web-services).

## The signs / symptoms / plans split

A strongly-held design principle.
Signs separate machine-oriented data (where mass analysis and automation rule) from symptoms (human-reported, discussion-heavy) and plans (developer proposals for change).
The split may be soft at the code level but is important at the data model and UI level.

Key motivation: duplicating two symptoms because they share a fix loses triangulation data if the fix is later found to address only one.
Plans decouple "what users experience" from "what we intend to change".

See [ADR-002](Decisions.md#adr-002-signs-symptoms-plans-separation).

## Web UI layer

A modern JavaScript front-end consuming the REST/JSON API.
The original design specified Handlebars templates; current alternatives include React, Vue, Svelte, and HTMX.
Choice should follow the language/stack decision (see [ADR-008](Decisions.md#adr-008-language-stack-open)).

Requirements regardless of framework:

- Server-side rendering of static views for search engine indexing
- Websocket or Server-Sent Events (SSE) for real-time client updates
- Accessible markup (WCAG 2.1 AA minimum)

## Atom feeds

Atom feeds remain a valid choice for content syndication and allow efficient indexing by search engines.
[JSON Feed](https://www.jsonfeed.org/) is a simpler alternative if Atom's XML is a maintenance burden.

## Push notifications: WebSub

[WebSub](https://www.w3.org/TR/websub/) (the W3C standardisation of the original PubSubHubbub/PSHB protocol, W3C Recommendation 2018) provides server-to-server push over HTTP.
It allows any subscriber to watch any feed without prior bilateral registration.

**Note:** WebSub adoption outside the IndieWeb community is limited.
For server-to-server push, webhooks are more widely understood and implemented.
Review whether WebSub is worth the complexity versus a simpler webhook-based notification system.
See [ADR-007](Decisions.md#adr-007-push-notifications-via-websub-formerly-pubsubhubbub).

For browser clients, a stateless proxy daemon subscribes to WebSub (or polls) and delivers updates via websockets or SSE.
This keeps the HTTP API server stateless.

## API specification

The API is defined in `.proto` files.
An OpenAPI spec is generated from the proto definitions and published for third-party integrations.
Proto service definitions are the contract at component boundaries, making the backend language an implementation detail per component.
The choice of RPC framework (Connect, grpc-gateway, tonic, etc.) follows the language decision in ADR-008.
Real-time browser updates use Server-Sent Events (SSE), which is browser-native and requires no client library.
See [ADR-009](Decisions.md#adr-009-api-specification-format--protobuf).

## Security

No security model was defined in the original design.
This must be resolved before any networked deployment.
Key areas: authentication (OAuth 2.0 / OIDC for browsers; API tokens for CLI), authorisation (role-based ACLs per repository), HTTPS everywhere, and a private disclosure workflow for security vulnerabilities.
See [ADR-010](Decisions.md#adr-010-security-model-open).

## Deployment

A fully functional public instance must run on a $10/month commodity VPS (1–2 vCPU, 1–2 GB RAM).
This is a design goal, not a post-hoc validation: architectural decisions that would require more resources at modest scale should be reconsidered.

The baseline deployment modes are:

- **Local dev:** Docker Compose (one command to start all services)
- **Production:** OCI container images; Docker or Kubernetes depending on scale
- **Minimal/single-node:** single binary (if the language choice supports it) or a simple `systemd` unit

No external managed services should be required for a basic self-hosted deployment.

## Language and stack

Not yet decided.
See [ADR-008](Decisions.md#adr-008-language-stack-open).
