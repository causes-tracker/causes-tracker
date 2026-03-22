# Architecture Decision Records

Decisions extracted from [Raw-Discussion.md](Raw-Discussion.md), [Manifesto.md](Manifesto.md), and [Design-Choices.md](Design-Choices.md).
Each record follows the format: **context → decision → status → consequences**.

---

## ADR-001: Distributed and replicated data model

**Status:** Accepted

**Context:** A centralised model is simpler to build but prevents offline use, private/behind-firewall deployments, and integration with external data sources (crash databases, downstream trackers).
The team had extensive experience with Launchpad's centralised model and its limitations.

**Decision:** Support both distributed and replicated operation.
Not all data sets need to be replicated everywhere (e.g. crash reports may be too voluminous), but the architecture must accommodate local repositories that sync with a remote.
Replication is pull-based: instances initiate outbound connections to fetch from upstreams.
This means instances do not need to be publicly reachable; any instance behind NAT can replicate from a public upstream.

**Consequences:** Increased implementation complexity.
Conflict resolution and eventual consistency must be designed in from the start.
Enables offline-first operation and private deployments without a separate codebase.
Pull-based replication also enables GitHub integration: a Causes instance can pull issue and PR data from GitHub's API and represent it in the Signs/Symptoms/Plans model.

---

## ADR-002: Signs / Symptoms / Plans separation

**Status:** Accepted

**Context:** Traditional bug trackers conflate machine-gathered crash data (signs), human-reported issues (symptoms), and proposed fixes (plans) into a single "bug" entity.
This causes friction: duplicating two symptoms because they share a fix loses the ability to triangulate the problem if the fix turns out to be wrong.

**Decision:** Model these as three distinct entity types.
Signs are machine-oriented and typically processed in aggregate.
Symptoms are human-authored and discussion-heavy.
Plans are developer-authored proposals for change, linked to one or more signs or symptoms.

**Consequences:** Richer data model but more cognitive overhead for users new to the system.
The UI must guide users to the right entity type.
See Manifesto.md for the full rationale.

---

## ADR-003: No hard dependencies on proprietary or hosted-only web services

**Status:** Accepted

**Context:** The project must support private/air-gapped deployments.
Any dependency on an external hosted service (e.g. a specific SMTP relay, a cloud push service) would prevent this.

**Decision:** Only use components for which a fully offline or self-hosted implementation exists.
The reference implementation must be deployable with no external service dependencies beyond standard protocols (HTTP, SMTP).

**Consequences:** Excludes some convenient hosted services.
Pushes toward standard protocols (Atom, SMTP, webhooks) over proprietary APIs.

---

## ADR-004: API-first; CLI builds on the same API

**Status:** Accepted

**Context:** Keeping the CLI and HTTP API as separate code paths leads to divergence and duplication.

**Decision:** The CLI is a client of the same local HTTP API that remote clients use.
The HTTP daemon provides API services; the CLI calls it.
This keeps layers clean and makes the API a first-class deliverable.

**Consequences:** The HTTP daemon must be running for CLI operations, or the CLI must embed the same logic.
Local-only mode needs consideration.

---

## ADR-005: Open source licence

**Status:** Accepted

**Context:** The team has day jobs and no interest in competing commercially with Atlassian, GitHub, Linear, etc.

**Decision:** Apache 2.0 or MIT licence.
Final choice between them is not yet made but both are acceptable.
Contributions must be dual-licensed under both to allow downstream users to choose.

**Consequences:** No commercial restrictions.
Anyone can fork and self-host.

---

## ADR-006: Federation strategy — distribute, don't federate by default

**Status:** Accepted (with caveats)

**Context:** Full peer-to-peer federation (as in ActivityPub) is complex and hard to get right.
The team discussed whether upstream/downstream relationships (e.g. a distro tracking upstream bugs) require true federation or can be handled by asymmetric replication.

**Decision:** Default to distribution (pull everything from a remote into a local repository).
True federation (pushing selected data upstream) is supported but not the primary model.
Downstream chooses what to push upstream; upstream can pull everything if desired.

**GitHub integration is required for incremental adoption.**
Projects will not migrate from GitHub unless Causes can pull from GitHub's issue and PR data and represent it in the Signs/Symptoms/Plans model.
The pull-based replication model (ADR-001) applies here too: a Causes instance pulls from the GitHub API rather than requiring GitHub to push to it.

**Consequences:** Simpler than full federation.
Downstream operators have control.
Does not preclude ActivityPub-style federation in the future.
GitHub is the de-facto centre of gravity for open source; treating it as a first-class upstream source is necessary for adoption.

---

## ADR-007: Push notifications — gRPC streaming

**Status:** Supersedes the original WebSub decision

**Context:** The original design used PubSubHubbub (PSHB, later WebSub) for server-to-server push, motivated by sublinear fan-out at large subscriber counts.
Realistic instance counts make sublinear scaling unnecessary.
A subsequent revision replaced WebSub with webhooks and SSE, but webhooks require the receiver to have a public URL — incompatible with the all-behind-NAT deployment goal (ADR-010).
Since all connections in the system are client-initiated outbound, gRPC streaming is the natural fit: the client opens a stream to the server and receives a push of events over it.

**Decision:** Use gRPC streaming for all server-push between non-browser parties.

- **CLI → instance:** gRPC server streaming for real-time event delivery; unary gRPC for commands.
- **BFF → instance:** gRPC server streaming for events the BFF fans out to browser clients; unary gRPC for API calls.
- **Federated instances:** gRPC bidi streaming; the downstream instance opens the connection to upstream and both parties can push changes.
  This is the natural expression of the pull-based replication model (ADR-001).
- **Microservices → instance:** gRPC unary or server streaming depending on the operation.
- **Browser → BFF → instance:** The BFF converts the gRPC event stream from the instance into SSE for the browser.
  SSE is the only non-gRPC protocol in the system, confined to the BFF↔browser edge.

Webhooks are removed from the core model.
They may be offered as an optional outbound integration mechanism for external tools that cannot speak gRPC, but are not load-bearing.

**Consequences:** One protocol (gRPC) for all non-browser communication; all types defined in proto.
No webhook delivery infrastructure (retry queues, signature verification, endpoint management).
All connections are client-initiated outbound.
Components that are intentionally public-facing still need a reachable endpoint: an upstream instance accepting federation connections, and a BFF serving internet users.
Private downstream-only instances (company mirrors, local dev) need no public endpoint at all.
The BFF is the single SSE conversion point; all other streaming is typed gRPC.

---

## ADR-008: Language / stack — OPEN

**Status:** Open — decision required before implementation

**Context:** Discussions in 2012–2013 mentioned Haskell, Clojure, Python, and PyPy as possibilities.
No decision was recorded.
The `.gitignore` in this repo suggests Haskell/Cabal was considered at some point.

**Decision:** _Not yet made._

**Options to consider:**

- **Python** (broad contributor base, good web frameworks, fast prototyping)
- **Go** (easy deployment as a single binary, strong concurrency story, good for API servers)
- **Rust** (performance, safety, single binary — but steeper learning curve)
- **Haskell** (original preference of some contributors — strong type safety, but small contributor pool)
- **TypeScript/Node** (large ecosystem, full-stack possibility)

**Criteria:** Easy for contributors to onboard, produces easily-deployed binaries or containers, has mature web framework and test ecosystem.
The architecture must support components in different languages communicating over the shared proto API (ADR-009); the initial language choice must not prevent per-component rewrites in faster languages later.

---

## ADR-009: API specification format — Protobuf

**Status:** Accepted — reversible (2-way door)

**Context:** The design specifies a RESTful JSON API.
Protobuf provides a language-agnostic schema with strong typing and multi-language code generation, which fits the distributed, multi-implementation nature of the project.
Because components may be implemented in different languages (see ADR-008), the proto definition is the contract at component boundaries — language is an implementation detail per component.

**Decision:** Define the API in `.proto` files.
Generate an OpenAPI spec as a published artifact via `protoc-gen-openapi` for third-party integrations and documentation.
The choice of server-side RPC framework (Connect, grpc-gateway, tonic, etc.) is left to ADR-008 once the backend language is known.

**Client types and their protocols (see also ADR-007):**

- **BFF → instance:** The only path for human browser sessions (see ADR-010).
  gRPC for API calls and server streaming for events; the BFF converts the event stream to SSE for browsers.
  The BFF and instance may be bundled in one binary but are structurally distinct layers.
- **CLI → instance:** gRPC with API tokens obtained via OIDC Device Flow (see ADR-010).
  gRPC server streaming for real-time event delivery (e.g. watching a live analysis job).
- **Microservice → instance** (GitHub connector, analysis tasks, etc.): gRPC with API tokens.
  These services initiate outbound connections; neither party needs to be publicly reachable.
- **MCP server → instance:** gRPC with pre-configured API tokens.
  A local MCP server adapts the instance's API for LLM tool use; gRPC streaming delivers live change events.
- **Federated instance ↔ instance:** gRPC bidi streaming; downstream initiates, both sides push.

**Consequences:** Component boundaries defined by proto services make per-component language rewrites possible without changing callers.
SSE handles all real-time push to connected clients regardless of client type.
OpenAPI is a generated artifact, not the source of truth.
If a specific RPC framework proves problematic, proto definitions remain valid and the framework can be swapped.

---

## ADR-010: Security model

**Status:** Partially decided — transport and auth flows settled; authorisation model open

**Context:** The original docs have no mention of authentication, authorisation, or security hardening.
For a system that can hold private bug data (e.g. security vulnerabilities), this is a critical gap.
Instances may be behind NAT with no public URL, which rules out OIDC redirect-based flows unless a BFF with a public URL mediates them.

**Decision:**

**Transport:** HTTPS mandatory for any networked deployment.

**Human authentication — OIDC Device Flow by default:** The user is directed to a public IdP URL (e.g. `github.com/login/device`); the BFF or instance polls for the token.
No redirect to the BFF or instance is needed, so this works whether or not either is publicly reachable.
Device flow is the default for both browser sessions (via BFF) and CLI.
Redirect-based OIDC social login is an optional enhancement for deployments with a public URL, not a baseline requirement.

**Browser access — BFF only:** Browsers do not connect directly to the instance.
A Backend-for-Frontend (BFF) layer sits between the browser and the instance.
The BFF may be bundled in the same binary as the instance but is a structurally distinct layer.
The BFF holds a service-account API token for the instance; the browser authenticates with the BFF via device flow.
The BFF itself may also be behind NAT — no public URL is assumed at any layer by default.

**Service-to-service authentication — API tokens:** Microservices, the MCP server, and the BFF authenticate to the instance with pre-issued API tokens.
No human flow involved.

**Authorisation:** _Not yet decided._
Areas to define: role-based model (user / developer / project admin), per-repository ACLs, and a private disclosure workflow for security vulnerabilities (private plans/symptoms visible only to authorised parties until disclosed).

**Consequences:** The BFF requirement means there is no "minimal" deployment that exposes the instance API directly to browsers.
The BFF and instance can be the same binary to keep deployment simple; the structural separation is what matters for security reasoning.
Device flow covers both CLI and any future non-browser client that needs human authentication.

---

## ADR-011: Build system — Bazel + BuildBuddy

**Status:** Accepted

**Context:** The repository will contain at least three distinct build targets: a TypeScript web UI, a CLI (command-line API client), and a backend server in a memory-safe systems language (Go or Rust, per ADR-008).
Builds must work efficiently on developer machines (Windows, macOS, Linux) and in CI (GitHub Actions).
Remote caching is required to keep CI fast as the codebase grows.
The system must remain usable by both human contributors and AI coding agents.

Options evaluated: Bazel + BuildBuddy, Buck2, Nx, Moon, Earthly, Gradle.
Earthly was abandoned by its maintainers in mid-2025.
Gradle has no genuine build-graph support for Go or Rust.
Buck2 has a small external community and no managed caching offering.
Moon (v2.0, February 2026) has first-class polyglot support but only ~79 contributors versus Bazel's 1000+; too new for a project that values stability.
Nx is well-suited to TypeScript-primary monorepos but has no Rust plugin as of early 2026.

**Decision:** Use [Bazel](https://bazel.build) as the build system with [BuildBuddy](https://www.buildbuddy.io) for remote caching and build observability.
Use [Bazelisk](https://github.com/bazelbuild/bazelisk) to pin and auto-install the correct Bazel version — contributors never install Bazel directly.
Use [Gazelle](https://github.com/bazel-contrib/bazel-gazelle) to auto-generate and maintain `BUILD` files for Go targets (eliminates the main maintenance burden for Go).
Use `rules_ts` for TypeScript and `rules_rust` for Rust if Rust is chosen (ADR-008).

**Windows strategy:** `rules_rust` disables its own Windows CI tests due to lack of maintainer expertise.
Windows developers building Rust targets must use a Dev Container (VSCode Remote Containers or GitHub Codespaces) which provides a Linux environment.
Windows developers working only on TypeScript targets may build natively.
If the backend language decision (ADR-008) lands on Go rather than Rust, the Windows constraint disappears — `rules_go` is CI-tested on Windows.

**Remote caching:** BuildBuddy free tier (10 users, 100 GB cache transfer/month, up to 80 remote execution cores) is sufficient for early development.
Self-hosted `bazel-remote` is an alternative if the free tier is outgrown or data residency is a concern.

**Consequences:** Bazel's hermetic, reproducible builds make CI reliable and agent-friendly — any machine produces identical outputs.
BUILD file authoring has a learning curve; Gazelle reduces this significantly for Go.
`MODULE.bazel` (bzlmod) is the current dependency management approach; some rulesets still have rough edges with bzlmod.
If scale demands it in future (remote execution, very large codebase), the same Bazel infrastructure scales without a migration.
