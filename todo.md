# TODO

**Current goal: NexQ's own REST API, and then everything that is built on top of it —
the CLI, the web UI, metrics, and the search backends.**

M1–M8 are done and have been removed from this file; the `aws sqs` CLI drives NexQ end
to end against the in-memory backend, single node, over HTTP or HTTPS. What was learned
building it lives in [crates/nexq-api-aws/README.md](crates/nexq-api-aws/README.md), in
the code, and in the tests — not here. Use `git log` for the narrative.

Everything below follows one dependency chain, which is why it is in this order:

```
REST (the contract)  ->  CLI          } both generated from one OpenAPI spec
                     ->  web UI       } so neither hand-maintains a second contract
                     ->  metrics      } which needs the counts REST already reports
                     ->  search store } the first backend that is not in-process
```

Deliberately **out** of scope until those are done: SNS, multi-node HA, the SQS ingest
bridge, KEDA's gRPC scaler, Docker/Helm. Sketched at the bottom.

**Sequencing note, flagged rather than silently reordered.** This order puts the search
backends (M14) ahead of SQLite/Postgres (M15), and that has two costs worth knowing
about before committing to it:

- Every store is gated on the same conformance suite, and search is the harder of the
  two to get through it — no transactions, so claim has to become a compare-and-swap,
  and a write is not immediately searchable. Doing it first means the first durable
  backend is also the one that has to invent the claim protocol, with no easier backend
  having proved the suite is passable by anything but the in-memory store.
- M12 and M13 put an operator in front of a queue whose entire contents disappear on
  restart. Fine for a demo, awkward as the state a web UI and a metrics endpoint exist
  to show.

Neither is a reason not to proceed — the search backends are the differentiating ones
and the memory store is genuinely enough to build a UI against. But pulling M15 forward
ahead of M14 is a one-line change to this file and it would remove both costs.

---

## M9 — The REST facade, contract-first

The plan's Q12 makes REST the complete surface, not a second-class one: the core
engine's operation set is the single source of truth, the SQS facade is a
compatibility-only subset of it, and **no capability may exist only behind SQS**. So
this milestone is parity plus the plumbing, and M10 is the extensions REST exists for.

Q18a is the constraint that shapes it: the spec is generated from the route and type
definitions, and every client — the CLI, the SPA, any published SDK — is generated from
that one spec. Nothing downstream is hand-written, so nothing downstream can disagree.

- [x] `[rest_api]` config: `enabled`, `bind_addr`, and `[rest_api.tls]`, sharing
      `ServerTlsConfig` and so `nexq-core::tls` — which is where the loader already lives
      precisely so this facade gets that one rather than a second copy. Startup validation
      is shared too, factored into `ServerTlsConfig::validate(section)` so each facade
      names its own keys: a bad REST certificate says `rest_api.tls.certificate` and never
      mentions `aws_api`.

      Defaults to `0.0.0.0:8081`, enabled — one past the SQS facade, so a config holding
      nothing but credentials serves both without either having to be moved first. A test
      asserts the two defaults differ, so a later change cannot quietly make them collide.

      A separate **port** rather than a path prefix, because the SQS protocol puts its
      operation in a header and posts everything to `/`: the two cannot share a listener
      without one of them giving up its natural URL shape. It also lets one facade be
      firewalled or TLS-terminated differently from the other, which is the usual reason to
      want SQS internal and REST reachable.

      TLS is **per facade and not inherited**, in either direction. More verbose when both
      want the same certificate, and still right: plain HTTP on a private interface for SQS
      while REST serves the network over TLS is a configuration an operator should be able
      to write, and inheritance would turn "switch on TLS here" into "switch it on
      everywhere".

      No `public_base_url`, and no account id, region, or clock skew — most of `[aws_api]`
      exists to satisfy AWS. A REST resource is addressed by name in the path rather than by
      a URL the server hands out and the client must send back. A comment marks where one
      would go if the OpenAPI `servers` list or a `Location` header turns out to need it.

      Two things beyond the item. Both facades on one `bind_addr` is now a startup error
      naming both keys — not hypothetical, since a port collision while checking this
      produced exactly the failure it prevents: `Address already in use (os error 98)`,
      naming neither the facade nor the setting, and which facade loses depends on the order
      they happen to be bound. Only *equal* addresses are caught; `0.0.0.0:8080` against
      `127.0.0.1:8080` collides too and is deliberately left to the OS, since deciding
      whether two socket addresses overlap means reimplementing the kernel's wildcard and
      dual-stack rules, and getting that subtly wrong would refuse configurations that work.
      And `Config::is_tls` is gone: it meant `aws_api.tls.is_some()`, had no callers, and at
      the top level cannot say which facade it is about — with two of them it reads as "this
      deployment serves TLS" while answering something narrower.

      **`nexq-core::tls::server_config` is not called for REST yet**, because there is no
      REST listener to build a `ServerConfig` for; that arrives with the server below. What
      is done here is the config surface and its startup validation.

      7 tests. The three that carry the weight were each checked to go red against a
      deliberate mutation: the clash check removed, the default port moved onto 8080, and
      the REST certificate error made to name `aws_api`.
- [x] Both facades in one process, over one `Engine`. `nexq-server` binds both when both
      are enabled, handing each the *same* `auth` and `engine`, and
      `nexq-api-rest/tests/cross_facade.rs` proves what that buys over real sockets:
      created and sent through the SQS facade with a genuine SigV4 signature, then received
      through REST — asserting the **message id**, not just the body, so it is the same
      message rather than a lookalike.

      Three more claims, because shared state is weaker than it sounds. **A message claimed
      over REST is invisible to SQS** — two facades could read one store and still both
      hand out the same message. **An SQS send wakes a REST long poll**, which is the
      strongest available proof the waiter registry is shared rather than duplicated.
      **Neither facade serves the other's routes**, and the refusal is routing rather than
      authentication, so a 404 there is not evidence a token was accepted.

      Deliberately not `oneshot` against the routers: two bound listeners is the claim, and
      a test that skipped the sockets would pass even if `nexq-server` never bound the
      second one. Verified by giving REST its own engine, which fails four of the five.

      What no test covers is `main.rs` itself, since it is a binary with no test target.
      Checked by hand — both `listening` lines appear, REST answers `401` without a token
      and `queue_not_found` with the real one from `nexq.toml` — and automating it belongs
      to the acceptance-suite item below.

- [x] Bearer auth: `<key_id>.<secret>` against the same credential registry the SQS facade
      signs against, per Q10b and the promise already made in
      [nexq.example.toml](nexq.example.toml). Constant-time comparison of the secret, and a
      `401` carrying `WWW-Authenticate: Bearer`. Not SigV4 — that constraint exists only to
      satisfy unmodified AWS SDKs, and nothing here is one.

      **Brought forward from below its place in this list**, because the item above could
      not ship safely without it: a receive endpoint is a queue-draining endpoint, and
      `rest_api.enabled` defaults to true. A supervisor that bound an unauthenticated one
      would have put "anyone on the network can drain your queues" into the default config
      for as long as it took to reach this line.

      A `layer` rather than a per-handler extractor, so adding a route cannot accidentally
      add an unauthenticated one. **One `401` for both an unknown key id and a wrong
      secret**, unlike the SQS facade's `InvalidClientTokenId` versus
      `SignatureDoesNotMatch`: that facade must distinguish them because AWS clients report
      on the distinction, this one has no such obligation, and not distinguishing them
      means a caller cannot enumerate which key ids exist. A test asserts the two responses
      are identical.

      The comparison's limits are written down rather than implied: length is still
      compared first, and an optimizer could in principle undo the loop. That is the trade
      for not adding a `subtle` dependency, and the length of a secret is not the secret.

      Also documented where it matters: the token is presented in full on every request, so
      unlike SigV4 — where a signature covers one request and the secret never crosses the
      wire — anyone who can read the traffic can replay it. That is why `[rest_api.tls]`
      exists.
- [x] Axum + `aide`: `ApiRouter`, and `#[derive(JsonSchema)]` beside the existing
      `Serialize`/`Deserialize`, so route registration *is* the documentation source. The
      alternative considered and rejected in Q18a was `utoipa`, whose per-handler
      `#[utoipa::path(...)]` amounts to writing the OpenAPI structure by hand.

      aide 0.15.1, which generates against **`schemars` 0.9** — not the `1` this file's
      placeholder guessed at. A `JsonSchema` from a different major version is a different
      trait and would not satisfy aide's bounds, so the two are pinned together.

      Served at `/api/v1/openapi.json`, pre-serialized once at router build time and handed
      out as refcounted `Bytes`. Readable **without a token**, deliberately: it describes
      the shape of the API and carries nothing deployment-specific — no queue names, no
      data — and a client generator has to be able to fetch it.

      Not the `scalar`/`redoc`/`swagger` features, which serve a documentation UI. Each
      renders a page that pulls its JavaScript from a CDN, which is exactly what an
      air-gapped deployment (Q21) cannot do. The spec itself is served instead.

      **Doc comments now have two audiences**, which is the part worth remembering. They
      are the published contract, so they are written for an API consumer and rationale for
      maintainers goes in plain `//` comments. Three things had leaked before the tests for
      them existed:

      - `[`MAX_MESSAGES_PER_RECEIVE`]` reached the spec as literal rustdoc syntax.
        `published_descriptions_are_not_written_for_rustdoc` now walks every description in
        the document and refuses one containing `` [` ``.
      - `Path<String>` produced an operation with **no parameters at all** — aide learns a
        path parameter exists but not what it is called. A `QueuePath` struct fixes it, and
        the field name is the parameter name.
      - The request body was documented `required: true` although the handler takes
        `Option<Json<..>>` and an empty `POST` is valid; aide's own `Option` input impl
        carries a TODO for exactly this. `receive_docs` corrects it.

      The limits are in the **schema**, not only in prose — `minimum`/`maximum` a generated
      client can validate against. They have to be literals, since an attribute cannot read
      a constant, so `the_documented_limits_match_the_engine` asserts they equal
      `MAX_MESSAGES_PER_RECEIVE` and `MAX_RECEIVE_WAIT`.

      Nine document tests, four verified against mutations: `Path<String>` back again, the
      body marked required, the documented maximum drifted to 25, and `route` in place of
      `api_route` — that last one is the important case, a route served but undocumented,
      and it fails four tests at once.

      One thing that is *not* proven: `openapi` resets aide's thread-local generation
      context, and removing that reset leaves every test green, because one route means one
      set of types and re-extracting them yields identical components. Kept as insurance
      with the comment saying so, rather than claimed as covered.
- [ ] The generated spec is **committed**, and a test fails when the committed copy and
      the generated one differ. Without that, the contract can change in a way no diff
      shows, and every generated client changes with it silently
- [ ] Resource-shaped routes, not a transliteration of SQS. Queue *name* in the path
      rather than a queue URL in a parameter — the URL-as-identifier is an AWS artifact
      that exists because SQS has accounts and regions to encode. Paging keeps the M2
      cursor tokens, since keyset paging is the store's guarantee and not a wire detail

      Settled already, since the one existing route had to choose: every route lives under
      **`/api/v1`**, held in `server::API_PREFIX` and applied by `Router::nest` so it is
      written once and a route cannot be added outside it by accident. Versioned because a
      generated client's base path should not have to change the first time the API does;
      under `/api` as well because the SPA will be served from the same origin and will want
      `/` and its own asset paths, and a queue named `assets` must not be able to collide
      with them. Tests spell the literal path rather than building it from the constant, so
      changing the prefix is a failing test rather than a silent break — six of them fail on
      a mutation back to bare `/v1`.

      Still open here: the rest of the surface, and paging, which has no route to page yet.
- [x] One error envelope: HTTP status, a stable machine-readable code, and a message,
      nested under `error` so a successful response and a failed one can never be told
      apart only by which fields happen to be present. Distinct from the SQS facade's
      `__type` shape, and mapped from the same `EngineError` values — every variant, so the
      two facades cannot disagree about what went wrong.

      ```json
      { "error": { "code": "queue_not_found", "message": "no queue named jobs" } }
      ```

      Codes are `snake_case` and part of the contract, since a client may branch on one;
      `message` is for a human and may change freely. The fallback answers in the same
      envelope, so a client parsing errors need not special-case a wrong URL.

      One variant is treated unlike the rest: a `Backend` failure is the only one that is
      *this server's* fault, so its detail is logged and withheld. A backend error can
      carry hosts, paths, or credentials, none of which belong in a response — there is a
      test that a `postgres://user:hunter2@db` connection string does not reach the client.
- [ ] Parity surface: create/get/list/delete queue, get and set attributes, purge,
      send/receive/delete/change-visibility, and the three batch operations. Idempotent
      creation is inherited from the engine rather than re-decided
- [x] Long-polling receive on the **same** `nexq-core::waiters` primitive as SQS — one
      mechanism with two protocol faces, per Q3a, not a second implementation. REST's
      `wait_time_seconds` goes to `Engine::receive` and nothing else, so there is nothing
      here to keep in step. `0` stays meaningful and distinct from omitting the field,
      which means the queue's own configured wait; a test pins the difference.

      The shutdown path too: `serve` calls `begin_draining`, and a poll parked for twenty
      seconds returns its ordinary empty answer rather than being waited out or dropped.

      **The first version of that test passed for the wrong reason.** Breaking REST's drain
      on purpose left it green — because both facades share the engine and both get the
      signal, so the *SQS* facade's `begin_draining` was releasing REST's waiters and the
      test could not tell which one had. It now binds REST alone, where nothing else can be
      the cause, and the same mutation fails. A green test that cannot go red is decoration,
      and this one nearly was.
- [ ] `nexq-client` generated from the spec, and used by something in-tree, so "the
      generated client works" is a fact rather than an assumption
- [ ] **Gate: `cargo xtask acceptance-rest` — a `curl`-level suite that exercises the
      surface over the wire, plus the same round trip through the generated client.** Two
      callers because a hand-written HTTP call proves the protocol and a generated one
      proves the spec describes it. Checked to go red, as with the M7 suites

## M10 — Priority, position-in-queue, DLQ and redrive

The features REST exists to expose. All three are unreachable from any AWS facade, which
is why they waited for one that can reach them.

- [ ] **Priority.** `Priority` and the store's priority ordering have existed since M3
      and no facade can set or read them — REST can. Default stays the middle of the
      road, so an unset priority behaves as it does today
- [ ] **Position in queue.** `Store::position_of`, per-backend by construction (Q7): an
      index into an ordered structure for memory, a count query for SQL and search. Two
      semantics to settle and then *document*, because both answers surprise someone:
      whether position counts only currently-visible messages, and that a
      higher-priority arrival moves you backwards. Approximate by nature, and named
      `Approximate…` for the same reason SQS names its counts that way
- [ ] **Dead-letter queues.** A DLQ is its own queue, first-class rather than a special
      case (Q8), so it already has a backend setting — defaulting to its source's,
      overridable, which is the point: a live queue on `memory` with its DLQ on something
      durable is a sensible configuration and needs no new concept
- [ ] A redrive policy on the source — `maxReceiveCount` and a target — enforced where
      the claim lapses, since that is the moment a delivery count increments. Needs
      `Store` support and conformance cases; a DLQ that only moves messages when someone
      happens to call receive is one that quietly does not
- [ ] Redrive back out: move messages from a DLQ to a target queue, inspectable and
      cancellable while it runs
- [ ] The four SQS operations deferred **with** DLQ in M6, now that there is something
      for them to report on: `ListDeadLetterSourceQueues`, `StartMessageMoveTask`,
      `CancelMessageMoveTask`, `ListMessageMoveTasks`. Plus `RedrivePolicy`, which
      `GetQueueAttributes` currently refuses with a stated reason
- [ ] Message expiry and `MessageRetentionPeriod`, **promoted from Future** because the
      question it was waiting on is answered here: an expired message is dropped or
      dead-lettered, and there was no DLQ to send it to before now. A sweep rather than a
      per-message timer, so the cost scales with queues and not with messages
- [ ] **Gate: a message that fails `maxReceiveCount` times lands in its DLQ, is visible
      there, and can be redriven back — over REST, and reported by
      `GetQueueAttributes` over SQS.** Both facades, because M9's promise is that the
      extended engine stays one engine

## M11 — The `nexq` CLI

Q13: a complete, self-sufficient replacement for `aws-cli`, not a supplement to it. The
earlier framing — "operators can just use the real `aws` CLI for standard operations" —
assumed `aws-cli` is available, which is exactly what does not hold in the closed and
air-gapped environments this project targets.

- [ ] Built on `nexq-client` **alone**, as the workspace dependency comment already
      states, so the binary stays small and drags in no server code
- [ ] `aws-cli`-style conventions per Q15: verb-noun commands (`create-queue`,
      `get-queue-position`), and `--output table|json|text`. JMESPath `--query` is
      explicitly the lower-priority part of "style" and can come later
- [ ] Full surface, extensions included — it is generated from the same spec, so
      completeness is close to free and a gap is a bug rather than a scoping decision
- [ ] Endpoint and token configuration: a profile file plus `NEXQ_ENDPOINT` /
      `NEXQ_TOKEN` overrides, matching the server's own file-plus-env convention (Q23)
      rather than inventing a second one
- [ ] `[client_tls]` finally gets its **first consumer**. M8 built and tested it with
      nothing using it, deliberately; this is the milestone that proves the loader was
      right, including a self-signed authority, which is the normal case on-prem
- [ ] No cluster awareness, per Q14 — the CLI connects to whatever node it is pointed at
      and that node proxies. Worth stating in the code, since "the CLI should find the
      primary" is the intuitive wrong answer
- [ ] **Gate: an acceptance suite driving the built `nexq` binary the way
      `acceptance-cli` drives `aws`** — the same operations, run against a real server,
      plus a check that the binary needs nothing at runtime beyond itself

## M12 — Web UI

Angular with [Optimus UI](https://optimus.openng.org/installation), already decided
below. `ui/` exists and is empty. Its client is generated from M9's spec, so the UI is
layout and wiring rather than contract work.

- [ ] Two decisions to settle first, both flagged under "Decisions still open" because
      they are not obvious: **how the browser authenticates** (the bearer token is a
      long-lived `<key_id>.<secret>` and putting that in `localStorage` is a real
      exposure), and **whether `cargo build` may require Node** (Q21's air-gapped
      constraint says the answer is no, which means either an off-by-default cargo
      feature or a committed bundle)
- [ ] Generated client from the committed spec, in the same codegen step as any other
      client. A hand-written API layer in the SPA is the exact duplication Q18a exists to
      prevent
- [ ] Served as embedded static assets by the REST facade, with an SPA fallback route —
      one binary, one port, nothing to deploy separately, which is the whole point for
      the single-VM target
- [ ] Views, in the order they earn their keep: queue list with depths, queue detail with
      attributes and message peek, DLQ management with redrive, and position lookup.
      Cluster status and bridge management have nothing behind them yet
- [ ] **Gate: create a queue, send to it, watch it appear, drive a message to the DLQ,
      and redrive it — entirely from the browser, against a binary built with no network
      access**

## M13 — Prometheus metrics

Q16 sequences this ahead of KEDA's gRPC scaler deliberately: the endpoint is cheap, it
serves general observability rather than only KEDA, and KEDA's `prometheus` scaler can
read it as-is. The gRPC external scaler is the larger, KEDA-specific lift and waits.

- [ ] Where it listens is a decision, not a detail: a separate `[metrics]` `bind_addr` is
      the convention, and it matters here because `/metrics` publishes **queue names and
      depths** — putting that unauthenticated on the public listener hands out the
      topology
- [ ] Queue depth per priority as a gauge, which is the series KEDA scales on and the
      reason priority is in the model at all
- [ ] The M6 counts as gauges (visible, not-visible, delayed), DLQ depth, counters for
      enqueue/claim/ack/expiry/dead-letter, a receive-wait histogram, a gauge of parked
      long-poll waiters, and build info
- [ ] A scrape must not cost a full scan. `message_counts` is an aggregate that is nearly
      free in memory and is not on a real backend, so the cheap-aggregate requirement
      belongs in the `Store` contract **before** M14 and M15 implement against it —
      otherwise a 15-second scrape interval quietly becomes the heaviest query the
      backend serves
- [ ] **Gate: `promtool check metrics` passes, and a KEDA `prometheus` ScaledObject
      scales a deployment off a real queue's depth** — documented as a working example
      rather than asserted to work

## M14 — OpenSearch and Elasticsearch backends

One crate for both, as `nexq-store-search` already says: they share nearly all of their
wire protocol. This is the first backend that is not in-process, and the first to make
the conformance suite earn its existence.

- [ ] **The conformance suite is the gate and it passes unmodified.** If a case needs
      relaxing to accommodate this backend, that is a finding about the contract to be
      settled explicitly, not a test to loosen
- [ ] Claim as a compare-and-swap via `if_seq_no`/`if_primary_term` — the same optimistic
      concurrency primitive Q1a picks for the leadership lease. There are no
      transactions, so "hand this message to exactly one consumer" has to be built from
      conditional writes
- [ ] Refresh visibility, which is the trap: a write is not immediately searchable, and
      the conformance suite assumes read-your-writes throughout (send, then receive).
      `refresh=wait_for` on writes buys correctness at a latency cost, and where that
      cost is paid should be a deliberate choice rather than a default nobody chose
- [ ] `position_of` as a count query (Q7), and `message_counts` as an aggregation cheap
      enough for M13's scrape interval
- [ ] `[client_tls]`'s second consumer, plus authentication to the cluster — these are
      *outbound* credentials, the same category the plan is careful to keep separate from
      NexQ's own trust root
- [ ] CI needs a service container for the first time, which **breaks the property every
      suite has had so far**: no secrets, no services, runs unchanged on a pull request
      from a fork. Worth keeping the rest of CI free of it rather than accepting the
      regression everywhere
- [ ] **Gate: the conformance suite green against a real OpenSearch and a real
      Elasticsearch, and verified to go red against a broken claim** — a distributed
      store that cannot fail the suite has not been tested by it

---

# Future

- FIFO Queues
- AWS Query/XML protocol
- Queue tagging — `TagQueue`, `UntagQueue`, `ListQueueTags`, and the tags `CreateQueue`
  already refuses. Needs nothing of the engine: a string map on a queue, which NexQ
  would store and attach no meaning to. The reason to build it is
  infrastructure-as-code, since a Terraform `aws_sqs_queue` sets tags and would fail
  without it; there is no reason beyond that, which is why it is here rather than in a
  milestone.
- `SenderId` on receive, which needs the sending principal recorded on the message.
  Named explicitly it is refused rather than answered with a placeholder, and `All`
  stays silent about it.
- Per-principal authorization — the registry authenticates _who_ a caller is, but
  every authenticated principal can do everything. Rules like "this consumer may
  receive from `jobs` but not purge it" need a permissions model, and are the reason
  per-principal keys are the recommended default now rather than later.
- Bound a long poll's sleep by the store's next visibility time. A message becoming
  claimable _without_ an enqueue — a delay elapsing, or a claim lapsing — wakes nobody,
  so a waiting consumer can find out up to one wait period late. Measured against the
  real `aws-cli`: a 2-second delay took **9.8s** to reach a consumer polling for 8, and
  a lapsed claim took 9.7s.

  Deliberately not urgent, because the gap closes itself in the cases that matter. It
  is tail latency and never starvation — the consumer's own poll loop is the retry, and
  the second poll returned in under a second both times. It only bites on an _idle_
  queue: with any other traffic the enqueues wake the consumer, it re-checks, and a
  3-second delay was picked up at 3.7s. And half of it is the failure path, since a
  lapsed claim means a consumer already crashed or hung, next to which 20 seconds is
  noise.

  Also deliberately **not a timer**. A timer would have to fire at every
  `claim_expires_at`, and in the happy path the consumer acks long before that — so
  almost every one of those timers would wake a consumer that finds nothing. Its cost
  would scale with _messages_ while its benefit scales with _waiting consumers on idle
  queues_, which is backwards. The cheap version is one `Store::next_visible_at` query
  per long poll that finds the queue empty, sleeping `min(deadline, next visible)`:
  nothing on the hot path, O(1) per waiting consumer rather than per message, and it
  needs one new `Store` method — `MIN(visible_at)`, indexable in SQL — plus a
  conformance case.

  M9 adds a second facade calling the same waiter primitive, which does not change the
  analysis but does double the number of callers that would benefit.

- `DelaySeconds` precision on a low-volume queue. The feature works, but a delayed
  message is picked up when a waiting consumer next looks rather than when its delay
  elapses, per the item above. Worth watching rather than ignoring: delayed retry with
  backoff is a common pattern, and it is the one case where somebody would actually
  notice work landing seconds late. Fixed by the same change; needs nothing of its own.

---

## After these milestones

Rough order, not committed:

1. **M15 — durable backends: SQLite, then Postgres**, against the conformance suite.
   Below M14 only because the requested order puts the search backends first; see the
   sequencing note at the top for why pulling it ahead is worth considering.
2. **SNS facade** — facade-only fan-out per Q11, a publish becoming N enqueues into
   per-subscriber queues. The core engine never learns pub/sub.
3. **Multi-node HA** — lease election, transparent proxying, rehydration on failover.
   Needs a network-accessible backend, so it needs M14 or M15 first.
4. **Docker image and Helm chart** — both deployment modes as first-class (Q22), and
   nothing pulled at runtime (Q21).
5. **The SQS ingest bridge** (Q24) — the forwarding loop is small; the runtime
   management surface across REST, CLI, and web UI is the bulk of it, which is why it
   lands after all three exist.
6. **KEDA external-scaler gRPC service**, once metrics and the engine are stable.

## Decisions made

- **`Store` trait dispatch: `dyn`.** Backends are behind `dyn Store` rather than a
  generic parameter or an enum, so a queue can name its backend at runtime and the
  server does not need one type per combination. Costs a vtable call and boxed futures
  per operation, which is nothing next to the storage round trip it wraps.
- **Credentials: the registry holds many, and the recommended posture is one per
  principal.** `[[auth.credentials]]` is already a list, so this is a documentation
  and defaults question, not an architecture one. Per-principal keys mean a single
  consumer's key can be revoked without touching everyone else's, and the principal
  name already logged on every request identifies who actually made it. NexQ does not
  yet act on _who_ a principal is — see the authorization item under Future.
- **REST auth: bearer `<key_id>.<secret>`, not SigV4** (Q10b), against the same
  credential registry. SigV4 exists to satisfy unmodified AWS SDKs and nothing on the
  REST side is one.
- **REST framework: Axum + `aide`** (Q18a), so route registration is the spec source.
  `utoipa` is more adopted but wants a hand-written `#[utoipa::path(...)]` per handler,
  which is the thing being avoided; `poem-openapi` is more automatic still but means
  adopting `poem` for the whole project.
- **Web UI: Angular with [Optimus UI](https://optimus.openng.org/installation)**
  (`ng add @openng/optimus-ui`), which needs Angular v21+ (v22 for Optimus v2) and
  RxJS 7.8.1+. Its component set — forms, tables, panels, charts — covers what the
  queue/cluster inspection and DLQ management views need, so the UI work is layout
  and API wiring rather than component building. Does not affect the backend: the SPA
  is generated from the OpenAPI spec and served as embedded static assets.

## Decisions still open

Three now, all in M12/M13 and none blocking M9:

- **How the browser authenticates to REST.** The bearer token is a long-lived
  `<key_id>.<secret>` pair; keeping it in `localStorage` means any XSS is a permanent
  credential theft rather than a session. A short-lived token exchanged at login, or an
  `HttpOnly` session cookie for the UI's own origin, are the obvious alternatives, and
  both add a concept the API does not have yet. Blocks M12, not M9.
- **Whether `cargo build` may require a Node toolchain.** Q21's air-gapped requirement
  says no, which leaves an off-by-default `ui` cargo feature or a committed built bundle.
  The first keeps the repo clean and makes the default build UI-less; the second makes
  every UI change a binary diff.
- **Where `/metrics` listens.** A separate port is the convention and here it is also a
  disclosure question, since the endpoint publishes queue names and depths.

Not blocking: per-principal authorization, still the next real architectural fork, and
still not needed by anything above.
