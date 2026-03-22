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

**Status:** Partially decided — transport, authentication, and several sub-areas sketched; authorisation model and several implementation details open

**Context:** The original docs have no mention of authentication, authorisation, or security hardening.
Causes handles sensitive data (security vulnerability disclosures, private plans) so the security model is load-bearing, not an afterthought.
Instances may be behind NAT; different instances will not share a private CA, ruling out SPIFFE/SPIRE-style workload identity for cross-instance machine authentication.
The federated, distributed nature of the system creates unusual requirements around identity, trust, and data sovereignty.

---

**Decided:**

**Transport:** HTTPS mandatory for any networked deployment.
When the BFF and instance run in the same binary, the gRPC between them is in-process (no TLS needed).
When they are separate processes on the same host, loopback is acceptable without TLS.
When they are on separate hosts, TLS is required.

**Human authentication — OIDC Device Flow by default:** The user is directed to a public IdP URL (e.g. `github.com/login/device`); the BFF or instance polls for the token.
No redirect to the BFF or instance is needed, so this works whether or not either is publicly reachable.
Device flow is the default for both browser sessions (via BFF) and CLI.
Redirect-based OIDC social login is an optional enhancement for deployments with a public URL, not a baseline requirement.

When device flow completes the IdP returns an ID token to the *instance* (not to the user's client).
The instance extracts the stable `(iss, sub)` pair from the ID token — not the email address, which can change and must not be used as a primary identifier.
The instance maps `(iss, sub)` to a local user record, creating one on first login for open instances (admin-approval mode is available for closed instances).
The instance then issues its own opaque session token to the client; the IdP token is held internally and never transmitted to the client.
The instance's token is subject to the HMAC replay protection described below and to its own revocation list.
Revocation at the IdP (e.g. a user revoking a GitHub OAuth grant) does not automatically cascade to the instance session; the instance's own revocation is the control plane.
A user may link multiple social identities (different IdPs) to a single local account via an explicit admin or user action; the `(iss, sub)` index supports this naturally.

**Browser access — BFF only:** Browsers do not connect directly to the instance.
The BFF may be bundled in the same binary as the instance but is a structurally distinct layer.
The BFF holds a service-account API token for the instance; the browser authenticates with the BFF via device flow.
Neither the BFF nor the instance is assumed to have a public URL.

**Service-to-service authentication — API tokens:** Microservices, the MCP server, and the BFF authenticate to the instance with pre-issued API tokens.
No SPIFFE/SPIRE: different Causes instances will not share a private CA, making shared PKI infrastructure impractical at the $10/month target.

Token auth is hardened as follows:

- **Scoping:** tokens are issued with explicit permission scopes (e.g. read-only, write, admin); the server rejects operations outside a token's scope.
- **Expiry:** tokens have a configurable expiry; long-lived tokens require an explicit admin decision and are logged as such.
- **Opaque values:** tokens are opaque random strings, not self-describing JWTs. Claims are resolved server-side on each request, so a revoked token cannot be verified offline.
- **Revocation:** the server maintains a revocation list checked on every request. Revoked tokens are rejected immediately, not at expiry.
- **Rotation:** a new token can be issued before the old one expires; a configurable overlap window allows zero-downtime rotation.

**Replay protection at the gRPC layer:**
Bearer tokens alone are vulnerable to replay: a captured request can be retransmitted.
Timestamp metadata alone is insufficient: an attacker who captures a request can replace the timestamp with a fresh one, since nothing binds the timestamp to the token.
The solution is HMAC request signing so that the timestamp and request ID cannot be altered without invalidating the signature.

Each API token has an associated signing secret (a symmetric key stored server-side alongside the token; never transmitted).

For every authenticated gRPC call the client interceptor computes:

```
HMAC-SHA256(signing_secret, request-id || timestamp)
```

and sends three metadata fields (RFC 6648 deprecated `x-` prefixes; these use an application namespace instead):

- `causes-request-id` — a UUID generated per call.
- `causes-timestamp` — current Unix time in seconds.
- `causes-signature` — the hex-encoded HMAC.

The server interceptor, applied to all authenticated endpoints:

1. Looks up the token, retrieves its signing secret.
2. Recomputes the HMAC over the received request-id and timestamp; rejects if the signature does not match.
3. Rejects if the timestamp is outside a configurable window of the server's clock (default ±30 seconds).
4. Rejects if the request-id has been seen within the same window (short-lived ring buffer of recent IDs).

Because both fields are covered by the HMAC, an attacker cannot modify either without knowing the signing secret.
The window limits the replay surface even for unmodified captures.

For streaming RPCs the stream handshake is signed as above; individual messages within an established stream carry a per-stream sequence number.
The server rejects duplicate or out-of-order sequence numbers within a stream.

**The instance as a mini authorization server:**
The instance implements a subset of OAuth 2.0 (RFC 6749) including the device authorization grant (RFC 8628), token introspection (RFC 7662), and token revocation (RFC 7009).
It acts as an authorization server for all clients — both human users (who authenticate against external IdPs and receive an instance-issued token) and services (which authenticate directly against the instance).
This means every token in the system — human or service — is issued and managed by the instance using the same mechanisms.

**Service account sessions:** Services (connectors, the BFF, background tasks) authenticate to the instance with service session tokens issued via the instance's own device authorization endpoint.
No token copying is required; services self-register by initiating a device flow and an admin approves via CLI or web UI.

Provisioning flow:
1. The service starts and calls the instance's device authorization endpoint, presenting a service name and description.
2. The instance returns a `user_code` and `verification_uri` pointing to the instance's own approval interface.
3. The service logs these to stdout (e.g. `Visit http://instance/device and enter code XXXX-YYYY to authorise this service`).
4. An admin with CLI or web frontend access approves the pending request (`causes auth approve XXXX-YYYY` or equivalent web UI action).
5. The service polls the instance token endpoint until approval; the instance issues a service session token.
6. The service stores the token; subsequent restarts reuse it without human intervention.

Service sessions are distinguished from user sessions by a `session_type` field set at creation and immutable thereafter.
Different policies apply: service sessions have longer or no expiry by default, carry no browser-cookie attributes, and are not subject to user-facing re-authentication prompts.
Service sessions carry: service name, description, and the identity of the admin who approved them.
The audit log records the session type alongside the identity for every action.
Service sessions can be revoked by any instance admin; the service re-provisions automatically on its next startup by repeating the device flow.

---

**Sketched — detail required before implementation:**

**Authorisation model:** Role-based with per-project ACLs.
Proposed default roles: anonymous (read public data only), authenticated user, developer (can create plans), project maintainer, security-team member (sees embargoed content), instance admin.
Instance admins can define custom roles with specific permission sets.
Role assignment: instance admin for global roles; project maintainer for project-scoped roles.
No privilege escalation: a maintainer can grant up to but not exceeding their own permission level.
_Open: exact permission matrix, API-level enforcement, and the role definition API._

**Federation trust and remote identity:**
Trust between instances is established explicitly by an administrator; there is no automatic trust.
Proposed mechanism: the admin of the downstream instance adds the upstream's public endpoint and verifies an out-of-band fingerprint (trust-on-first-use with admin confirmation).
After trust is established, the upstream issues a federation API token to the downstream.
Remote identities (users from another instance) are represented as opaque federated references: `(instance_url, local_id)` pairs.
Only a display name is stored locally; email addresses and other personal data are not replicated across instance boundaries.
If the remote instance is unreachable, previously stored display names remain available; further resolution fails gracefully.
_Open: key rotation after compromise, revocation of federation trust, trust transitivity policy._

**Private disclosure workflow:**
Signs, symptoms, and plans may be marked embargoed at creation time by any authenticated user, or promoted to embargoed by a security-team member after the fact.

Embargoed content:
- Is not federated to any downstream instance, regardless of federation configuration.
- Is not visible to users without the security-team or project-maintainer role.
- Does not appear in search results, feeds, notifications, or public API responses.

Embargo lifecycle:
1. Reporter marks content embargoed (or a security-team member does so after the fact).
2. Security team triages and investigates; embargoed plans are created linked to the embargoed symptom.
3. Fix is developed. The plan may reference a private branch or external patch tracker.
4. A project maintainer or security-team lead lifts the embargo. All affected content becomes visible simultaneously.
5. Federation of the previously embargoed content proceeds normally after disclosure.

Default embargo period: 90 days, configurable per project.
Extensions require an explicit action by a maintainer or security-team lead; silent expiry is not permitted.
_Open: CVE assignment integration, coordinated multi-project disclosure._

**GDPR and personal data:**
Causes stores personal data including display names, email addresses (from IdP tokens), user-generated content, and audit logs.
The GitHub connector imports data from GitHub's API that may include email addresses and real names; these are subject to the same handling rules as locally-entered data.

Key principles:
- **Minimisation:** email addresses are stored for authentication; they must not be exposed in the general API.
- **Right of erasure:** a user can request account deletion. Personal data is removed; content is deleted or reassigned to an anonymous placeholder. Content already federated to downstream instances cannot be guaranteed erased there — this must be documented to operators in the privacy policy template.
- **Portability:** users can export their own content.
- **Consent:** the privacy policy must be presented at sign-up.

_Open: audit log retention policy, data residency controls for federated deployments._

**Resistance to information harvesting:**
Email addresses must not be visible to unauthenticated users.
Authenticated users see email addresses only where explicitly required (e.g. admin user management), never in bug comments or general API responses.
The API returns opaque user identifiers in all public-facing responses; email resolution occurs only server-side.
In federated contexts, remote instances receive only the opaque federated identifier and display name of local users.
Authentication endpoints must be rate-limited to resist enumeration.
Failed authentication returns the same error regardless of whether the account exists.

**New instance bootstrap:**
First startup generates a one-time setup token, prints it to stdout, and expires it after first use or after a configurable timeout.
This token is used to create the initial instance admin account.
Secure defaults: fresh instances start with anonymous access disabled, no open federation, and no connectors configured.
The instance refuses to start with an invalid or incomplete security configuration.

**Configuration recovery:**
If the admin loses access: an emergency recovery token is generated at install time, stored separately from the database (e.g. written to a file at first boot), and usable once to regain admin access.
Its use always generates an audit log entry.
Non-secret configuration must be exportable for documentation and DR planning.
Secret files are excluded from configuration exports by path convention; operators back them up separately with appropriate access controls.

**Disaster recovery:**
Periodic database backup and point-in-time restore must be supported out of the box.
Default RPO: 24 hours for small deployments (daily backup).
Default RTO: time to restore from backup on equivalent hardware — no HA requirement at the $10/month scale.
Federated data may be partially recoverable from upstream instances after a restore; the system must document which data is local-only (higher DR risk) versus federated (potentially recoverable from upstream).
Operators running larger deployments can implement HA at the database layer without application changes.

**Connector credential management:**
Connectors hold external credentials (e.g. a GitHub App private key or personal access token) and a service session token for the instance.
All secrets are stored as plaintext in documented file paths with restricted permissions (readable only by the process user).
This is deliberate: Causes does not implement its own encryption at rest.
Operators who require stronger secret management (k8s Secrets, Vault, encrypted filesystems) can provide secrets via files — the same interface works with all of them.
Credential rotation: admins can update credential files and signal the connector to reload without service interruption.
Connectors must request only the minimum permissions needed from the external service.
GitHub connector specifically: GitHub App is preferred over personal access tokens (scoped permissions, no user-account dependency, rotation via private key rather than user secret).
All connector API calls are logged with the connector's identity.

**Consequences:** The BFF requirement means there is no deployment mode that exposes the instance API directly to browsers.
The BFF and instance can run in the same binary to keep deployment simple; the structural separation matters for security reasoning.
Device flow as the universal default means social login is an enhancement, not a dependency.
The embargo model requires the federation layer to inspect content sensitivity before replicating — federation cannot be a simple log replay.

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
