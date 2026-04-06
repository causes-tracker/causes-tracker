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

**Decision:** Apache 2.0.
Apache 2.0 is preferred over MIT because it includes an explicit patent grant, which protects contributors and users from patent claims on the covered code.

**Consequences:** No commercial restrictions.
Anyone can fork and self-host.
The patent grant is a meaningful protection as the project grows.

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
gRPC streaming handles all real-time push to non-browser clients; SSE is confined to the BFF→browser edge only (see ADR-007).
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

```text
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

_Instance identity:_
Each instance has a stable `instance_id` (UUID v4) generated at first bootstrap and written to its configuration.
The `instance_id` is independent of the contact URL — if an instance moves domains its identity is unchanged.
Each instance's local trust registry maps `instance_id → contact_url` for each trusted peer.

_Trust establishment:_
The admin adds the upstream's contact URL.
The downstream connects over TLS; PKI establishes the upstream's server identity — no out-of-band fingerprint step is required.
The downstream then initiates the device authorization flow against the upstream to register as a federated service account (see "The instance as a mini authorization server").
An admin on the upstream approves via CLI or web UI; the upstream issues a service session token and returns its `instance_id`.
Subsequent federation API calls are authenticated by this service account; the authenticated channel is the trust anchor for all content received from that upstream.

_Federation authorisation:_
All connections are initiated outbound by the connecting instance, but the receiving instance maintains a service account record for each remote instance even though it never connects back.
This record is the hook for authorisation: roles on the service account control what the remote instance may do.
Proposed permission axes: subscription scope (which projects may be replicated), publication permission (may the remote instance push resources here), and publication scope (which projects may receive pushed resources).
Roles are assigned by an admin when approving the device flow registration, and can be updated at any time without re-establishing the connection.
A downstream with publication permission pushes resources over the same outbound connection it uses for subscription; the upstream does not need to initiate any connection to receive pushed content.

_Worked example — private company instance and public open-source instance:_
The company operates a private instance; the open-source project operates a public instance.
The company registers a service account on the public instance with subscription permission (to receive community signs and symptoms) and publication permission scoped to the project's plan namespace.
The company creates plans internally, linking them to public symptoms; plans originate on the private instance and their journals are authoritative there.
When ready to share, the company pushes selected plans to the public instance.
Community members discuss the published plans on the public instance; those comments are new resources originating on the public instance.
The company's instance polls the public instance for new journal entries on its published resources, receiving community discussion back without the public instance needing to initiate any connection.

_Remote identities:_
Users from another instance are represented as `(instance_id, local_user_id)` pairs.
Only a display name is stored locally; email addresses and other personal data are not replicated across instance boundaries.
If the remote instance is unreachable, previously stored display names remain available; further resolution fails gracefully.

_Provenance:_
Every journal entry carries provenance:
- `origin_instance_id`: the `instance_id` of the instance that wrote this entry.
- `origin_id`: a stable UUID identifying the resource this entry is about.

`origin_id` is the globally stable identifier for a resource.
It is assigned once at resource creation and reused by all subsequent entries, regardless of which instance writes them.
Different entries for the same resource may have different `origin_instance_id` values.
Journal entries are deduplicated on `(origin_instance_id, origin_id, version)`.

The replication path (which upstream delivered an entry and any relay hops) is instance-local metadata populated by the receiver, not part of the immutable journal entry.
Trust flows from the authenticated service account of the upstream, not from the provenance record itself.

_Resource journal:_
Every resource (sign, symptom, plan, and related entities) has an append-only journal of entries.
Each journal entry records:
- `version`: monotone sequence number, assigned at commit time on the writing instance.
- `at`: timestamp.
- `by`: federated identity `(instance_id, local_user_id)` of the author.
- `kind`: one of `entry` or `tombstone`.
- `previous_version`: a federated version reference pointing to the prior entry for this resource (0 for the first entry).
- `snapshot`: full resource state at this version.
- `provenance`: as above.

Deletes are logical — a `tombstone` entry is appended; content is not physically removed.
Any instance that has replicated a resource may write entries about it.
The `previous_version` chain links entries across instances, capturing the full edit history.
See [ADR-013](Decisions.md#adr-013-replication-protocol) and [Replication.md](Replication.md) for the full protocol specification.
The journal is the replication unit: a downstream requests entries via a watermark-based cursor, enabling incremental catch-up without a full re-sync.
The journal also supports abuse handling: previous versions of edited or deleted content remain recoverable by instance admins.
The journal gives a precise meaning to "deletion across federation boundaries": a downstream that has replicated a resource retains its journal entries even after a `tombstone` entry is written.

In the UI, instances are displayed by hostname only (e.g. `causes.example.org`), with the scheme and path stripped.
No additional slug or display name field is required; the hostname is sufficiently human-readable for admin views and attribution labels.

_Open: key rotation after upstream compromise, revocation of federation trust._

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
_Deferred: CVE assignment integration, coordinated multi-project disclosure._

**Personal data handling:**
Causes stores display names, email addresses (from IdP tokens), user-generated content, and audit logs.
The GitHub connector imports data from GitHub's API that may include email addresses and real names.

The software must:
- Not expose email addresses in general API responses; email addresses are stored for authentication and visible only in explicit admin views.
- Support account deletion: personal data is removed and user-generated content is deleted or reassigned to an anonymous placeholder.
  Content already federated to downstream instances cannot be guaranteed erased there; the software must surface this caveat to the user at deletion time.
- Support user data export so a user can retrieve all content they have created.
- Allow operators to configure a policy URL or policy text shown to users at account-creation time.
  This is especially important for social-login flows, which have no device-flow interstitial where a consent step can naturally appear.

**Operating an instance:**
GDPR compliance obligations — lawful basis, data processing agreements, and handling subject access requests — rest with the operator of each instance, not with the Causes project.
Operators must provide a privacy policy and present it to users at sign-up; the software provides the mechanism (the configurable policy text/URL above) but does not supply the policy itself.
Operators must inform users in their privacy policy that content federated to other instances cannot be guaranteed erased on deletion.
Operators are responsible for configuring audit log retention appropriate to their jurisdiction and risk appetite.
Operators are responsible for data residency; the software imposes no constraint on where it is deployed.

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

---

## ADR-012: Proto API conventions — typed IDs, ResourceMeta, and federation provenance

**Status:** Accepted

**Context:** The Causes API is defined in Protocol Buffers.
The API must support federation: resources created on one instance can be replicated to others, and downstream instances must be able to create relationships between resources without write access to either resource's origin instance.
Identifier fields must carry enough type information to prevent cross-domain mistakes and to appear safely in URL path segments.
Common resource metadata (id, timestamps, embargo flag, origin) must be defined once and shared rather than duplicated across message types.
Federation provenance has two distinct aspects — stable origin identity and per-entry transit path — that must not be conflated.

**Decision — domain-narrowing wrappers:**
Where the domain of a field is narrower than its protobuf primitive type, use a named wrapper message rather than the primitive directly.
Each wrapper contains a single field carrying the underlying value (e.g. `string value = 1;`).
The wrapper type name documents the constraint and prevents cross-domain assignment at compile time.

Examples:
- `SignId`, `PlanId`, `UserId` and every other identifier field: a field typed `SignId` cannot accidentally receive a `PlanId`.
  All identifier values must conform to the URL-safe alphabet: ASCII letters (A-Z, a-z), digits (0-9), hyphens (-), and underscores (_).
  This ensures any ID can appear literally in a URL path segment without percent-encoding.
- Markdown body fields: a `Markdown` wrapper distinguishes fields whose string content is interpreted as Markdown from plain-text strings, enabling renderers and validators to select the correct code path without inspecting field names.

Language-specific generated code enforces value constraints at construction time.

When a field must carry one of several typed values (e.g. a Comment's parent may be a Sign, Symptom, or Plan), use proto3 `oneof` with a distinct typed field per case.
There is no exception to the wrapper rule: even when the underlying encoding is identical, the distinct type names preserve semantics and enable static checking.

**Decision — ResourceMeta:**
All top-level resources (Sign, Symptom, Plan, Comment, Annotation, and link types) embed a single `ResourceMeta meta = 1;` field rather than repeating id, project_id, timestamps, embargo flag, and origin individually.
This eliminates drift between resource types and gives readers a single place to look for common fields.

**Decision — federation provenance split:**
Two concepts that were previously conflated in a single provenance field are now separated:

- Entry origin (`origin_instance_id`, `origin_id`): carried on each journal entry.
  `origin_instance_id` identifies the instance that wrote the entry.
  `origin_id` is a stable UUID identifying the resource; it is assigned once at creation and reused by all subsequent entries regardless of which instance writes them.
  See [ADR-013](Decisions.md#adr-013-replication-protocol) for full details.

- Replication path: the transit path for a specific journal entry — which upstream delivered it and which relay hops it traversed.
  Per-entry: the receiver appends the sender to the path on ingest, building up the full delivery chain.
  Locally-authored entries start with a path of `[self]`.

**Decision — link objects for relationships:**
Many-to-many relationships between resources (Plan↔Sign, Plan↔Symptom) and ordering dependencies between Plans are stored as independent link objects (`PlanSignLink`, `PlanSymptomLink`, `PlanDependency`) in `link.proto`.
This allows a downstream instance to record a local link without requiring write access to either linked resource.
Link objects carry their own embargo flag; the server enforces the invariant that a link is embargoed whenever either linked resource is embargoed.

**Decision — Annotation vs Comment:**
Machine-generated or connector-supplied structured data on a Sign is stored as an `Annotation` (in `annotation.proto`) rather than a `Comment`.
Annotations carry a binary `content` field and an `annotation_type` string (convention: `"domain/version"`) that identifies the content schema.
Human narrative discussion belongs in `Comment` messages.
Comments may be attached to Signs as well as Symptoms and Plans.

**Decision — project_id is local filing metadata:**
`ResourceMeta.project_id` is always a local identifier on the storing instance.
It is not part of the stable resource identity — `ResourceOrigin` provides that.
Project structure is never replicated; there is no globally stable project identity.

Project mapping is the responsibility of the connection originator: the instance that establishes the federation connection.
For a downstream pushing resources to an upstream, the downstream (connection originator) translates its local project_id to the upstream's project_id before serialising the snapshot.
For a downstream pulling resources from an upstream, the downstream (connection originator) translates the upstream's project_id to its own local project_id as it stores the received snapshots.
In both cases the snapshot arrives at the storing instance already containing the correct local project_id; no rewriting on ingest is required.

This invariant holds transitively: in a chain of instances, each connection originator manages only the mapping between its own project IDs and its direct neighbour's.
No instance learns about the project structure of non-adjacent instances.

**Consequences:** The typed-wrapper approach adds message nesting versus plain strings, but the type-safety and constraint-documentation benefits justify the cost.
The `ResourceMeta` embedding pattern requires all resource messages to reserve field 1; this is a stable convention that Gazelle and code generators can enforce.
The `ResourceOrigin`/`ReplicationPath` split makes the data model more explicit at the cost of two separate fields where one existed before; the clarity is worth it.
Link objects add an extra round-trip to create a relationship but are the only approach that is consistent with the federation trust model.

---

## ADR-013: Replication protocol

**Status:** Accepted

**Context:** The distributed data model (ADR-001) and federation strategy (ADR-006) require a replication protocol.
The protocol must handle multi-instance topologies (A↔B→C), embargoed content, resource renames, and integration with external systems via connectors.
The full specification is in [Replication.md](Replication.md).

**Decision — version = transaction ID:**
Journal entry versions are assigned from the origin instance's transaction ID (or equivalent monotone-on-commit source).
This avoids explicit sequence allocation, row locks, and write contention.
Write transactions that create references must use snapshot isolation (REPEATABLE READ) or higher to guarantee topological ordering.

**Decision — version 0 as root sentinel:**
Version 0 is reserved and never assigned.
It is the root of every `previous_version` chain: a create entry has `previous_version = 0`.
A replication watermark of 0 means "start from the beginning."

**Decision — `previous_version` chain:**
Every journal entry carries `previous_version` — the version of the immediately prior entry for the same resource.
This enables gap detection (receiver can identify missing entries) and full audit trails.
Renames are not a special operation — they are entries where the slug changed, linked by the chain.

**Decision — per-project replication watermark:**
Downstream instances track one watermark per (upstream, project).
This is efficient (subscribe to specific projects) and scoped to the federation authorization model (ADR-010 proposes per-project subscription scope).

**Decision — at-least-once delivery with watermark:**
Commits can land out of version order (transaction T1 starts before T2 but T2 commits first).
Each journal entry stores a watermark — the oldest uncommitted transaction ID at commit time.
The replication watermark winds back to ensure no entries are skipped.
Entries already served are filtered by a bounded seen buffer.
Receivers deduplicate on `(origin_instance_id, origin_id, version)`.

**Decision — journal entries immutable, resources mutable:**
Journal entries are immutable once created (like git commits).
Resources are mutable — they can be renamed, updated, deleted, and undeleted (like git file paths).
The history of a resource is the chain of its journal entries linked by `previous_version`.

**Decision — embargo as journal property:**
The `embargoed` field is per journal entry, not per resource.
Embargoed entries are filtered during replication by default; the federation trust configuration controls per-peer access (see ADR-010).
Un-embargoing creates a new journal entry with `embargoed = false` carrying the full content; the `previous_version` gap is expected.
Embargo propagates transitively to references.

**Decision — causal dependencies via versioned references:**
Cross-resource references carry `(origin_instance_id, origin_id, version)` — a pointer to a specific journal entry.
The topological ordering of the replication stream guarantees referenced entries precede referencing entries.
This avoids the need for vector clocks or consensus protocols.

**Consequences:** The protocol is simple (one RPC, one watermark per project, at-least-once with dedup) but handles complex topologies correctly.
The topological ordering proof relies on snapshot isolation — implementations must enforce this.
The watermark mechanism adds two columns per journal row (Postgres-specific) but is invisible in the proto wire format.
