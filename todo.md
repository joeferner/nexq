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

## M10 — Priority, position-in-queue, DLQ and redrive

The features REST exists to expose. All three are unreachable from any AWS facade, which
is why they waited for one that can reach them.

- [ ] **Position in queue.** `Store::position_of`, per-backend by construction (Q7): an
      index into an ordered structure for memory, a count query for SQL and search. Two
      semantics to settle and then _document_, because both answers surprise someone:
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
      _outbound_ credentials, the same category the plan is careful to keep separate from
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
