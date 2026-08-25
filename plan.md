# Plan — NexQ: standalone distributed queue service

> **Status: pre-design.** No code written yet. This document exists to pin down scope and
> surface open questions before any implementation planning starts. Written in Rust, as its
> own standalone project (own repo, not part of any existing codebase).

---

## 1. High-level features

- Distributed, priority-aware job/message queue, low memory footprint (Rust)
- Pluggable storage backends: **Postgres, SQLite, in-memory, OpenSearch, Elasticsearch**
- Wire protocol facades, each independently enable/disable-able: **SQS-compatible**,
  **SNS-compatible**, and a native **REST** API with the extended feature set
  (position, priority, DLQ admin, cluster status) plus its own **long-polling**
  receive
- Per-message **priority**
- **Position-in-queue** query ("where am I in line")
- **Dead-letter queues** (DLQ) with redrive
- **Primary/secondary failover** via a backend-mediated lease (Raft dropped — see
  Q1a)
- Primary is the single source of truth for dequeue ordering (enables true blocking
  dequeue instead of client-side polling)
- **Prometheus** metrics endpoint
- **KEDA** integration for autoscaling consumers
- Simple **web UI** (queue/cluster inspection, DLQ management)
- **CLI** with command surface similar to `aws sqs` / `aws sns`
- **Docker image + Helm chart** so standing up a single-node or multi-node HA
  deployment is a pull-and-run/`helm install`, not a manual setup

---

## 2. Motivating problem

Standalone and on-prem deployments — no cloud provider, often air-gapped, sometimes
just a single VM or a small k3s footprint — need a distributed job/message queue, but
the common options don't fit well:

- Cloud-managed queues (AWS SQS/SNS) require a cloud account, which these
  deployments don't have by definition.
- Most self-hosted brokers (Redis+BullMQ, RabbitMQ, NATS JetStream) mandate their
  own dedicated infrastructure rather than working with whatever storage a
  deployment already happens to be running.
- None of the common options combine native priority, position-in-queue
  visibility, and dead-letter queues in one system.

Throughput requirements are low — this is not a high-scale streaming/Kafka-shaped
use case, it's ordinary job/task queuing where "distributed and easy to operate"
matters far more than raw speed.

Alternatives considered and rejected (see §4) rather than adopting NexQ as designed
here — Redis+BullMQ, storing the queue directly in OpenSearch/Elasticsearch, and
NATS JetStream — pushed toward a bespoke project instead because none of them
satisfy the backend-pluggability requirement (use whatever storage a deployment
already has, rather than mandating a new infra dependency) or native priority +
position + DLQ in one system.

---

## 3. Open questions needing answers

Roughly in the order they block downstream design:

### Architecture
1. **[ANSWERED] Is Raft used for leader election only, or does it replicate the queue
   log itself?** Confirmed: election only. The durable state lives in the chosen
   backend (Postgres/OpenSearch/Elasticsearch already handle their own
   durability/replication) — NexQ does not ship its own replicated storage engine.
1a. **[ANSWERED] Do we actually need Raft for this, or is a backend-mediated lease
   enough?** Decided: **no Raft, for the network-accessible backends.** Since leadership
   is *just* "which process is allowed to arbitrate dequeue order," and that state
   only needs to be visible to backend-connected nodes, the backend can hold the lease
   itself instead of running a separate Raft cluster/port among NexQ processes:
   - **Postgres** — an advisory lock, or a lease row a node claims/renews with a
     conditional `UPDATE ... WHERE expires_at < now()`.
   - **OpenSearch/Elasticsearch** — a lease *document* claimed via optimistic
     concurrency (`if_seq_no`/`if_primary_term`) — the same general
     compare-and-swap shape many systems use for a singleton-leader claim against
     a shared datastore, just applied here to leadership rather than data updates.

   Trade-off vs. real Raft: failover isn't instant — there's a gap up to the lease
   TTL where no primary is recognized, instead of Raft's heartbeat-driven near-
   immediate handover. Given queue throughput is explicitly not a design goal, and
   "easy to stand up and manage" favors fewer moving parts (no separate consensus
   port, no quorum-sizing decisions, no Raft crate dependency, nothing to get subtly
   wrong), a tunable lease TTL is probably the better default. Raft would only earn
   its complexity back if a customer needs sub-lease-TTL failover. **Decision: no
   Raft for now; build the lease-in-backend mechanism** — see the refinement below
   for how "for now" stays cheap to revisit later.

   **Refinement (supersedes the per-queue-backend leadership consequence noted
   under Q8): election mechanism is its own explicit `cluster` config, decoupled
   from any queue's data backend.** Rather than leadership being implicitly tied to
   whichever backend a given queue happens to use, one cluster-wide setting names
   how primary is decided — e.g. `cluster.leaderElection.backend:
   postgres | opensearch | elasticsearch | raft` — independent of what any
   individual queue is configured to store its data in. This directly resolves the
   multi-primary complexity Q8 raised: with one election mechanism for the whole
   deployment, there is exactly one primary cluster-wide, full stop, regardless of
   how many different backends individual queues use for data. The election
   backend's connection doesn't even need to overlap with any queue's data backend
   — an operator could point election at Postgres purely for that purpose while
   every queue's data lives elsewhere. `raft` is kept in the enum as a future value
   rather than dropped outright — same recommendation as before (`openraft`, not
   hand-rolled) if it's ever implemented — so "drop Raft for now" doesn't foreclose
   it later, it just isn't the default. Single-node deployments omit `cluster`
   config entirely (or set it to a `none`/absent value) — no election backend
   needed when there's only one process (Q3b).

   **Does lease polling reintroduce the job-dequeue polling problem (Q4/§4)?** No —
   different scale entirely. Job-dequeue polling was a risk because it scales with
   **number of workers/clients** (large, elastic, unbounded) against a backend that,
   for OpenSearch/Elasticsearch, might also be serving unrelated production
   search/query traffic. Lease checking scales with **number of NexQ server nodes**
   (small, fixed, operator-controlled — 2-5), and splits into three cheap cases: (1)
   the primary renewing its own lease — one periodic write from exactly one node,
   e.g. every `TTL/3`; (2) standbys checking whether the lease has expired so they
   can claim it — bounded by node count, and the interval can be coarse since
   failover speed was never a hard requirement (already accepting lease-TTL-scale
   failover, not sub-second); (3) a secondary proxying a client request (Q3b) only
   needs an on-demand, optionally cached read of "who's primary" at the moment a
   request arrives — no background loop at all for that path. Net: some extra
   backend traffic, but small, fixed-cardinality, and internal — not
   customer-facing like the job-polling case.
2. **[ANSWERED] Which backends support HA at all?** `memory` and `sqlite` are
   single-process by construction — no second node can observe the same state, so
   failover cannot apply to them regardless of mechanism. Only `postgres`/
   `opensearch`/`elasticsearch` are candidates for the primary/secondary topology.
   Confirmed as acceptable: single-node backends simply don't offer HA, and single-node
   *deployments* of NexQ are a first-class supported mode in their own right (see Q3b)
   — not a degraded fallback.
3. **[ANSWERED] How do secondaries handle client requests?** Transparent server-side
   proxy (see Q3a below) — not a client-side redirect.
3a. **[ANSWERED] How does a client connect to NexQ?** Confirmed, three facades, each
   independently enable/disable-able via config:
   - **SQS-compatible** and **SNS-compatible** — plain HTTP(S), exactly like talking
     to real AWS SQS/SNS; the client, or an unmodified AWS SDK, is configured with
     one endpoint URL, same as `--endpoint-url` today (see Q9). Scoped to whatever
     subset of the real API "SQS-compatible" customer expectations require (see §5
     non-goals) — not a goal to replicate the entire AWS API surface.
   - **REST** — NexQ's own native protocol, the only one exposing the extended
     feature set (position-in-queue, priority set/query, DLQ inspect/redrive,
     cluster/leader status — see Q12), and the only one that needs to support
     **long-polling receive**: a request that holds its connection open until either
     a job is available or a timeout elapses, then returns (empty on timeout, same
     semantics as SQS's own `WaitTimeSeconds`). This means REST's receive endpoint
     sits on the *same* blocking-dequeue primitive as the SQS facade's long-poll
     (Q4/§4's "one poller per primary, not per worker") — it's one core mechanism
     with two protocol faces, not two separate implementations.

     **[ANSWERED] Should a non-primary node proxy a long-poll REST request through
     for its full duration, or redirect the client to primary?** Decided: **proxy**,
     same as SQS/SNS — one consistent routing mechanism across all three facades,
     rather than a special-cased redirect path for REST alone. Accepted trade-off:
     every long-held connection is held twice (client↔secondary and
     secondary↔primary) for its full duration. Since the underlying server is async
     Rust, holding many idle connections is cheap relative to a thread-per-connection
     model — worth confirming this stays true under real long-poller counts once
     there's something to load-test, but not a reason to special-case REST at design
     time.
3b. **[ANSWERED] How does that connection get routed to the current primary?**
   Confirmed: **transparent server-side proxying, backed by the same lease record from
   Q1a.** Every node in a multi-node deployment listens on the same address/port; a
   node that receives a request while it isn't the leaseholder reads the current
   primary from the shared backend lease and forwards the request internally,
   relaying the response back. The client never needs cluster awareness. This is
   effectively required, not just convenient, given Q9's constraint that unmodified
   AWS SDKs must work — they don't do multi-node leader discovery, and HTTP clients
   don't reliably follow redirects on POST. An external LB/VIP that only routes to
   the primary (k8s readiness-gated Service; keepalived/VRRP on bare VMs) was
   considered as an alternative that avoids the proxy hop, but it adds a
   deployment-specific moving part outside the app — worse for "easy to manage,"
   especially on standalone/edge targets — so it should stay an optional deployment-
   level optimization, not the default routing mechanism.

   **Refinement: a failed proxy forward doubles as an early failure-detection
   signal.** When a node's forward to its cached last-known primary fails
   (connection refused/timeout), that's a cheap, immediate hint — on top of Q1a's
   periodic background lease-expiry check, not a replacement for it (a quiet cluster
   with no client traffic would never observe a failed proxy, so the periodic check
   is still needed as the correctness backstop). On a forward failure the node should
   **re-read the lease record**, not assume leadership outright — the failure could
   be a transient network blip between this node and a still-healthy primary, not an
   actual outage. The backend's conditional lease claim remains the sole arbiter of
   who is primary, so a spurious signal can't cause split-brain: the node only
   becomes primary if it also wins the conditional write, which fails if the lease is
   still validly held. In practice this means most real failures get detected (and a
   reclaim attempted) on the very next client request rather than waiting out the
   periodic poll interval — faster perceived failover with no added background load.
   While the gap is being resolved, the proxying node should return a retryable
   error (e.g. `503` + `Retry-After`) rather than hang, so unmodified AWS SDK retry
   behavior handles it without any client-side awareness of the failover.

   **Single-node mode is a first-class case, not a degraded HA mode**: when there is
   only one NexQ process, there is no lease to check and no proxying — that process
   is trivially always primary. This mode works with *every* backend, including
   `memory`/`sqlite`. Multi-node HA is opt-in and only available with a
   network-accessible backend (Q2), and the proxy/lease machinery above should be
   inert/absent entirely when running single-node, not merely a no-op path through
   the same code.
4. **[ANSWERED] How does blocking dequeue work per backend?** No per-backend
   mechanism needed at all — not Postgres `LISTEN`/`NOTIFY`, not a poll loop for
   OpenSearch/Elasticsearch. It falls out of a design decision already made: every
   enqueue is proxied to the current primary (Q3b), so the primary's own process is
   always the one performing the backend write — it already knows about a new item
   the instant it writes it, in-memory, with no discovery step required. It can hold
   long-poll/blocking-dequeue connections as in-process waiters (e.g. a broadcast
   channel or per-queue condition variable) and wake the relevant ones directly after
   its own write, re-evaluating priority order at wake time to decide who actually
   gets served. Same reasoning covers lease/visibility-timeout expiry (nack/redelivery):
   the primary granted the lease, so it already knows the expiry time and can schedule
   an in-process timer for it, instead of polling the backend to ask "did anything
   expire." This eliminates backend polling entirely for all five backends, not just
   the ones lacking native pub-sub.

   Two things this depends on, worth keeping visible so they don't silently break:
   - **It only holds because every write goes through the primary.** If a future
     change ever allowed a write to bypass the primary (direct backend access,
     an operator script, migration tooling), the primary wouldn't observe it until
     the next rehydration below. Not worth solving now — just don't casually
     introduce a bypass later.
   - **The backend write is still the durability source of truth; in-memory
     notification is a latency optimization on top of it, not a substitute for it.**
     If the primary crashes between writing to the backend and notifying waiters,
     nothing is lost — the write already landed. A newly-elected primary does a
     one-time **rehydration read** of current queue/lease state from the backend at
     the moment it takes over leadership (not a recurring poll), reconstructs its
     in-memory waiter/timer state, and resumes serving from there.

### Storage backend abstraction
6. **[DEFERRED] What does the common backend trait look like?** Not a blocking
   up-front design decision — expected to fall out during implementation. Rough
   shape sketched for reference, not a commitment: `enqueue(payload, priority)`,
   `claim_next() -> Job` (lease/visibility-timeout semantics), `ack`, `nack`/`retry`,
   `extend_lease`, `dead_letter`, `position_of(job_id) -> u64`, `stats()`.
7. **[DEFERRED] Is "position in queue" computed the same way across backends?**
   Also expected to fall out during implementation rather than be decided up front.
   Likely *not* uniform — e.g. an index into an ordered structure for `memory`/
   `sqlite` vs. a `count` query for `opensearch`/`elasticsearch` — but the exact
   per-backend approach is an implementation detail, not an architecture fork.
8. **[ANSWERED] Should each queue have its own backend configuration — and does
   that settle DLQ storage?** Yes: per-queue backend config, and a DLQ is simply
   modeled as its own queue (first-class, not a special case), which already has a
   backend setting — defaulting to match its source queue's backend, overridable
   (e.g. live queue on `memory` for speed, DLQ on `sqlite`/`postgres` so failed
   items survive a restart).

   ~~Consequence: leadership/lease election must be scoped per-backend, not
   per-deployment~~ — **resolved by Q1a's refinement**: election is its own
   explicit `cluster` config, decoupled from any queue's data backend, so there is
   exactly one primary per deployment regardless of how many different backends
   individual queues use for data. Mixing backends across queues no longer
   complicates "who is primary" at all.

### Protocols
9. **[ANSWERED] How faithful does the SQS facade need to be?** Confirmed: **the
   real `aws` CLI and any AWS SDK (any language), unmodified,** must be able to
   talk to NexQ via `--endpoint-url`/custom endpoint config. This is a general
   tool-compatibility bar, not scoped to any particular consumer. Practically this
   means covering the operations `aws-cli`/SDKs commonly exercise beyond
   send/receive/delete (e.g. `create-queue`, `list-queues`, `get-queue-url`,
   `get-queue-attributes`, `purge-queue`), not just the minimal produce/consume
   surface.
10. **[ANSWERED] Does the SQS/SNS facade enforce SigV4 signing?** Confirmed: yes,
    real verification, not just shape-tolerance. NexQ owns its own access-key-id →
    secret-access-key registry (unrelated to AWS IAM — NexQ is its own trust root)
    and recomputes the HMAC to compare against what the client sent. Note this
    requires storing secrets in a form usable to recompute the signature
    (encrypted-at-rest), not one-way-hashed the way passwords normally would be,
    since the server needs the actual secret value back, not just something to
    compare a hash against.

    **Operator workflow this implies**: NexQ issues a key/secret pair; the operator
    runs `aws configure --profile nexq` (or sets `AWS_ACCESS_KEY_ID`/
    `AWS_SECRET_ACCESS_KEY`) with it, plus *any* region string (SigV4 only needs
    signer and verifier to agree on the same value — it doesn't need to be a real
    AWS region), and points the endpoint at NexQ (`--endpoint-url`, or
    `AWS_ENDPOINT_URL_SQS` / `endpoint_url` under `services.sqs` in
    `~/.aws/config`). Every `aws sqs`/`aws sns` command under that profile then
    signs automatically — no extra flag beyond the endpoint override. NexQ should
    return the same error shapes real SQS does on a mismatch
    (`InvalidClientTokenId`, `SignatureDoesNotMatch`) so `aws-cli`'s own error
    reporting stays meaningful. (Not `aws sso login` — that's a separate,
    browser-based flow for temporary STS credentials; a static key/secret pair
    issued directly by NexQ is the right shape here, the same way LocalStack is
    normally used.)
10a. **[ANSWERED, resolved by 10b] Does NexQ need credential management, and is it
    one shared key or per-principal keys?** Confirmed needed — not an incremental
    cost anymore. Since 10b settles REST on API-key/bearer-token auth, NexQ is
    already issuing, storing, and validating credentials for that alone, so
    "does it need credential management" isn't a separate decision to make. The two
    facades' auth mechanisms differ (SigV4 signing for SQS/SNS vs. bearer
    presentation for REST), but both can likely be served by one shared underlying
    credential/principal registry rather than two independent stores — and because
    that registry naturally models "principal → credentials," per-principal keys
    fall out for free rather than being extra work over a single shared key. Whether
    to actually *default* to issuing one key per principal vs. one shared key for a
    deployment is a small follow-on default-behavior choice, not an architecture
    question.
10b. **[ANSWERED] What does the REST facade use for auth?** Decided: a simpler
    API key/bearer token, not SigV4 — REST has no AWS-SDK-compatibility reason to
    imitate AWS's signing scheme (that constraint only applies to SQS/SNS, per Q9),
    so it can use NexQ's own, simpler convention instead.
11. **[ANSWERED] Is SNS a first-class concept in the core engine, or a facade-only
    fan-out?** Confirmed facade-only: a "publish" on an SNS-compatible topic becomes
    N `enqueue` calls into per-subscriber queues, the same way AWS composes SNS→SQS
    fan-out. The core engine keeps one job model (durable, priority-ordered,
    competing-consumers) and never needs to understand pub/sub semantics itself.
12. **[ANSWERED] What does the plain REST API expose that the SQS/SNS facades
    can't?** Confirmed: the custom extensions (priority set, position query, DLQ
    redrive, cluster/leader status) — but REST is **not limited to just those**. It
    must also carry full parity with everything the SQS/SNS facades can do
    (send/receive/ack/create-queue/etc.), so a client writing against REST to get
    the extended features never needs a second, dual-protocol code path just to do
    basic produce/consume. Concretely: the core engine's operation set is the
    single source of truth; the SQS and SNS facades are each a compatibility-only
    translation layer exposing a *subset* of those operations in AWS's wire format,
    while REST is a complete, native mapping onto *all* of them, extensions
    included. No capability should ever exist only behind the SQS/SNS facade and be
    unreachable from REST.

### CLI
13. **[ANSWERED, revises earlier framing] Is a bespoke CLI needed for standard
    send/receive/create-queue operations too, or only the proprietary extensions?**
    Confirmed: the CLI must be complete, covering both the proprietary features
    *and* full standard-operation parity — not scoped down to extensions only, as
    earlier framing assumed. The earlier assumption ("operators can just use the
    real `aws` CLI for standard ops via `--endpoint-url`") quietly relied on
    `aws-cli` being available at all — which doesn't hold for customers in closed
    or air-gapped environments who may not have it installed or permitted. This
    doesn't add design work: since the CLI talks to REST (Q14) and REST now has
    full parity with SQS/SNS plus the extensions (Q12), a complete CLI falls out
    naturally from exposing all of REST's surface, rather than being a second
    implementation effort. Net effect: the bespoke CLI is a full, self-sufficient
    replacement for `aws-cli` for anyone who can't or doesn't want to use it, not
    just a supplement to it.
14. **[ANSWERED, corrects an inconsistency with Q3b] CLI transport** — the CLI
    talks to the same REST endpoints as any other client, no different from an SDK.
    It does **not** need to be leader-aware or follow the current primary itself —
    this earlier framing directly contradicted Q3b's decision that transparent
    server-side proxying means *no* client, SDK or CLI, ever needs cluster
    awareness. The CLI connects to whatever node it's configured with; that node
    proxies to the current primary internally if it isn't one itself. No
    SQS-wire parsing either way — it was already talking to REST, not the SQS
    facade.
15. **[ANSWERED] How closely should CLI command naming/output mirror `aws-cli`?**
    Confirmed: aws-cli-style for consistency — verb-noun command naming
    (`describe-queue`, `get-queue-position`) and familiar output-format
    conventions (e.g. `--output table/json/text`). This matters more now than when
    first raised, given Q13's decision that the CLI is a full drop-in replacement
    for `aws-cli` in closed environments — matching its conventions eases that
    transition for anyone coming from real AWS tooling. Full `--query` JMESPath
    replication stays the lower-priority, can-add-later part of "style" rather than
    a day-one requirement — the core value is naming/output familiarity, not
    matching every piece of `aws-cli`'s machinery.

### Observability / ops
16. **[ANSWERED] Which KEDA integration mode?** Both: NexQ should expose a
    Prometheus endpoint (queue-depth-per-priority as a gauge, trivial for KEDA's
    `prometheus` scaler to read) **and** implement KEDA's
    [external-scaler gRPC interface](https://keda.sh/docs/latest/concepts/external-scalers/)
    for push-based scaling decisions, rather than picking one. These are very
    different amounts of work, so worth sequencing rather than building both at
    once: the Prometheus endpoint is cheap and serves general observability too
    (not just KEDA), so it's the natural first target; the external-scaler gRPC
    interface is the larger, KEDA-specific lift and can follow once the core engine
    and metrics are stable.
17. **[SUPERSEDED by Q1a]** Raft crate choice (`openraft` vs `raft-rs`) is moot if
    leader election moves to a backend-mediated lease instead of Raft. Only revisit
    this if Q1a's recommendation is overturned by a real fast-failover requirement —
    and if so, `openraft` (async, tokio-native) over `raft-rs` (sync, TiKV's choice)
    given an async server, not hand-rolling consensus either way.
18. **[ANSWERED] Web UI tech**: embedded static SPA, confirmed. Which frontend
    framework it's built in (Angular, React, or otherwise) is explicitly deferred
    as a future, non-blocking decision — doesn't affect backend architecture.
18a. **[NEW, follows from Q18] The REST backend must be built as a contract-first
    library that generates an OpenAPI/`swagger.json` spec, which becomes the single
    source of truth for every generated client** — not just external SDKs for
    queue producers/consumers in whatever languages, but also the SPA's *own*
    client for talking to the backend. This mirrors a well-established
    contract-first pattern: generate the spec once from the backend's actual route
    and type definitions, then generate every client — external SDKs and the SPA's
    own client alike — from that single spec. One spec, one codegen step, no
    hand-maintained duplicate contract between backend and UI. Since REST is
    already the complete API surface (Q12 — full SQS/SNS parity plus every
    extension), this one spec covers everything, for every client, including the
    web UI.

    **[ANSWERED] Recommendation: Axum + `aide`.** Axum is the current default
    choice for new Rust APIs in 2026 — built and maintained by the Tokio team,
    idiomatic/composable, fits the async architecture already assumed elsewhere in
    this design (in-process waiters for blocking dequeue, lease renewal timers) —
    and picked up official JetBrains RustRover support this year, a good maturity
    signal. For OpenAPI generation specifically, given the stated preference for
    maximum type-binding/auto-generation and avoiding anything that amounts to
    hand-writing the OpenAPI structure: **`aide`** over `utoipa`. `aide`'s
    `ApiRouter` wraps `axum::Router` directly, so route registration *is* the
    documentation source — schemas come from `#[derive(JsonSchema)]` (via
    `schemars`) alongside the normal `Serialize`/`Deserialize` derives, and its
    0.5.0 rewrite explicitly dropped macro-based annotation in favor of tracing
    documentation from real source code. `utoipa` is far more adopted (~4k GitHub
    stars vs. `aide`'s ~670, more active, has an official `utoipa-axum` binding),
    but requires a `#[utoipa::path(...)]` macro per handler hand-specifying
    method/params/responses — closer to writing the OpenAPI structure by hand than
    deriving it, which is exactly what's being avoided here. `poem-openapi` is the
    most fully automatic option of all (routing and spec are the same declaration,
    no separate framework-routing layer) but means adopting `poem` as the web
    framework instead of Axum — trading away Axum's ecosystem dominance for the
    whole project, not just the API layer, which isn't worth it given Axum's
    current position. Accepted trade for choosing `aide`: smaller community/lower
    long-term-maintenance certainty than `utoipa`, worth revisiting only if that
    becomes a real problem in practice.

### Scope / rollout
19. **Phasing** — not yet decided (explicitly deferred by the requester as of this
    writing; this document is a design sanity-check pass, not a build-order plan).
    Candidate shape once it is decided: single backend + SQS facade + single-node
    (no Raft) first, to validate the SQS-compatibility acceptance test in Q9 before
    investing in clustering.

### Deployment
20. **[ANSWERED] Docker image + Helm chart, to make deployment a pull-and-run
    rather than a manual setup.** Confirmed: NexQ ships its own standalone Docker
    image and Helm chart — any Kubernetes-based deployment can `helm install` it
    directly, or add it as a chart dependency, without being coupled to any other
    project's structure or release cycle.
21. **[ANSWERED] Image build strategy.** Confirmed: the base image choice is
    flexible (Alpine or similar is fine) rather than requiring the most extreme
    minimal option (distroless/scratch) — a multi-stage build still applies (build
    in a full Rust toolchain image, copy just the compiled binary into the small
    runtime image), but there's no hard requirement to go all the way to scratch.
    The air-gapped/offline constraint stands regardless of base choice: the image
    must not pull anything at runtime, and should be easy to include in an offline
    bundling/mirroring process for disconnected environments — a real constraint
    given the standalone/edge deployment targets this project is meant to serve.
22. **[ANSWERED] The chart needs to model both deployment modes already designed —
    single-node and multi-node HA — as first-class, not bolt one on later.**
    Confirmed. Maps
    directly onto Q1a/Q2/Q3b: single-node omits `cluster.leaderElection` entirely;
    multi-node HA sets it to a network-accessible backend. Worth calling out an
    existing design payoff here: because every node proxies correctly regardless of
    which one receives a request (Q3b), a **plain `ClusterIP` Service with ordinary
    round-robin routing is sufficient** for the multi-node case — no
    readiness-gated Service, external LB, or VRRP/keepalived setup is needed just to
    route traffic to whichever node is primary, unlike the more complex options
    that were considered and deliberately not chosen for that purpose earlier.
23. **[ANSWERED] Config convention.** Rust-idiomatic: a config file (via e.g.
    `figment`/`config`) with env var overrides, rather than adopting some other
    project's env-var convention — as a standalone project, NexQ has no external
    deployment tooling to stay compatible with, so the natural default for a
    Rust service applies.

---

## 4. Alternatives considered

| Option | Why not chosen as the sole answer |
| --- | --- |
| **Redis + BullMQ** | Easiest to stand up, but a new infra dependency in every deployment; doesn't offer pluggable storage against whatever a customer already runs. Still a reasonable fallback/comparison point. |
| **Queue stored directly in OpenSearch/Elasticsearch** (infra a customer might already be running, zero new containers) | No native blocking dequeue (naive design = every worker polls, competing with any other production query load on the same cluster); claim contention under optimistic-lock polling scales with worker count; would require hand-building ack/nack/lease/DLQ/redelivery that a real queue library gives for free. NexQ's "one poller per primary, not per worker" design (Q4) is intended to fix the specific objection about polling load if OpenSearch/Elasticsearch is chosen as a NexQ backend. |
| **NATS JetStream** | Already provides Raft-based leader election, DLQ (via max-deliver redirect), Prometheus metrics, and an existing KEDA scaler, as a single small binary. Does not support pluggable storage backends (owns its own storage) or native per-message priority — the two most distinctive requirements here — so adopting it outright would mean giving up the backend-pluggability goal. |
| **RabbitMQ** | Native priority queues and mature clustering, but heavier footprint (Erlang runtime) and no per-message "position in queue" API; also fixed storage, not pluggable. |

---

## 5. Non-goals (for now)

- High-throughput/streaming use cases (Kafka-shaped problems) — explicitly not the
  target; "doesn't need to be super fast" was a stated requirement from the start.
- Exactly replicating every `aws-cli` SQS/SNS operation and output mode — only the
  operations real-world usage commonly exercises, plus whatever a customer
  reasonably expects from "SQS-compatible."
