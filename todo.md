# TODO

**Current goal: the unmodified `aws sqs` CLI drives NexQ end to end, against the
in-memory backend, single node.**

Nothing else is in scope until that works. It is the highest-risk part of the whole
design — SigV4 verification and AWS wire-protocol fidelity either work with real AWS
tooling or they don't — and it is also the smallest thing that proves the architecture,
since the SQS facade is a translation layer over the core operation set, so building it
exercises the engine and the facade boundary at once.

Deliberately **out** of this milestone: the REST facade, the `nexq` CLI, SNS, durable
backends, priority, position-in-queue, DLQ, clustering/HA, metrics, KEDA, web UI,
Docker/Helm. Those are sketched at the bottom and should stay untouched until
`aws sqs` works.

Target definition of done:

```sh
aws configure --profile nexq          # key/secret issued by NexQ, any region
aws --profile nexq --endpoint-url http://localhost:8080 sqs create-queue --queue-name jobs
aws --profile nexq --endpoint-url http://localhost:8080 sqs send-message --queue-url ... --message-body hi
aws --profile nexq --endpoint-url http://localhost:8080 sqs receive-message --queue-url ... --wait-time-seconds 10
aws --profile nexq --endpoint-url http://localhost:8080 sqs delete-message --queue-url ... --receipt-handle ...
```

---

## ✅ M1 — A signed request gets a valid response

Prove the protocol and auth path before writing any queue logic. `list-queues` on an
empty server is the smallest request that exercises all of it.

- [x] Config: per-facade listener sections plus a shared credential registry, as a
      TOML file with `NEXQ_*__*` environment overrides
- [x] Axum server in `nexq-api-aws`, owning its own listener, with graceful shutdown;
      `nexq-server` binds and runs it only when `aws_api.enabled`
- [x] Identify the wire protocol the installed `aws-cli` v2 actually sends — captured
      from `aws --debug sqs list-queues` against the running facade, see below
- [x] Request routing off `X-Amz-Target`, with JSON body decode — operations are a
      typed enum, so "not built yet" and "no such operation" are distinct answers
- [x] SigV4 verification: canonical request reconstruction, HMAC recompute, compare
      against the client's signature. Verified against a real botocore signature
      captured from `aws --debug`, so correctness does not rest on reading the spec
      correctly — see `reproduces_a_signature_that_botocore_computed`.
- [x] Reject with SQS's own error shapes: `InvalidClientTokenId`,
      `SignatureDoesNotMatch`, and the JSON protocol's `__type` envelope
- [x] Reject stale signatures with a clock-skew window — `RequestTimeTooSkewed`, within
      `aws_api.max_clock_skew_secs` (default 900, as AWS allows). Checked in both
      directions and before the signature is recomputed. `0` disables it, which the
      server warns about at startup, since that restores indefinite replayability.
- [x] `ListQueues` returning an empty list — an empty object, since real SQS omits
      `QueueUrls` when there are none and `aws sqs list-queues` then prints nothing
- [x] **Gate: `aws --endpoint-url ... sqs list-queues` succeeds, and fails correctly
      with a bad secret.** Against the real `aws-cli` 2.36.30: correct credentials
      exit 0 with no output; a wrong secret reports `SignatureDoesNotMatch`; an
      unknown key id reports `InvalidClientTokenId`; an unsigned request gets
      `403 MissingAuthenticationToken`. Any region string works, as designed.

What `aws-cli` 2.36.30 sends for `sqs list-queues`, captured against the facade:

```
POST / HTTP/1.1
X-Amz-Target: AmazonSQS.ListQueues
Content-Type: application/x-amz-json-1.0
x-amzn-query-mode: true
X-Amz-Date: 20260826T005924Z
Authorization: AWS4-HMAC-SHA256 Credential=AKIANEXQDEV/20260826/us-east-1/sqs/aws4_request,
  SignedHeaders=content-type;host;x-amz-date;x-amz-target;x-amzn-query-mode, Signature=...

{}
```

Notes worth pinning down here, since getting them wrong is silent and confusing:

- `x-amzn-query-mode: true` is sent on every request and is part of the signed
  headers. It asks for Query-protocol-shaped errors over the JSON protocol, so error
  responses have to account for it rather than assuming plain JSON error shapes.
- The credential scope names the service as `sqs`, and the region is whatever the
  client is configured with — SigV4 only needs signer and verifier to agree on it, so
  it need not be a real AWS region.
- The CLI surfaces a non-AWS error body verbatim: a plain 501 came back as
  `An error occurred (501) when calling the ListQueues operation: <body>`. Useful for
  debugging now, and a reminder that error shapes are what the CLI reports on.
- The Query/XML protocol still matters for older SDKs, but is deliberately deferred
  until the JSON path works.

## ✅ M2 — Queue lifecycle against the memory backend

- [x] Domain model: queue, message, receipt handle — `QueueName` validates on
      construction, `Message` and `ClaimedMessage` separate the durable item from a
      time-limited claim
- [x] `Store` trait, behind `dyn` via `async-trait`, covering queue lifecycle only —
      create, get, delete, list — plus a `StoreError` that separates "no such queue"
      from "the backend broke"
- [x] Memory store in `nexq-store-memory`, its own crate alongside the other backends
- [x] `nexq-store-conformance` suite covering the operations implemented so far,
      running green against the memory store. 25 cases generated as individual
      `#[tokio::test]`s by `conformance_tests!(new_store)`; verified to _fail_ by
      deliberately breaking five contract promises in the memory store
- [x] Core engine operations: create, get, delete, list — with idempotent creation
      decided here, so every facade inherits it
- [x] Queue URL construction _and parsing_ from the configured public base URL — the
      CLI sends every subsequent request to whatever URL `CreateQueue`/`GetQueueUrl`
      returns, so both directions have to agree; `QueueUrls` owns the format and a
      round-trip test pins it
- [x] The account id in queue URLs is config (`aws_api.account_id`), not a constant,
      defaulting to `000000000000` and validated as exactly 12 digits. Changing it
      invalidates queue URLs clients already hold, which the docs now say.
- [x] `CreateQueue`, `DeleteQueue`, `ListQueues`, `GetQueueUrl`, including
      `QueueNamePrefix` and the `VisibilityTimeout`/`DelaySeconds`/
      `ReceiveMessageWaitTimeSeconds` attributes
- [x] `QueueNameExists` / `QueueDoesNotExist` error parity, plus
      `InvalidParameterValue`, `InvalidAttributeName`, and `InvalidAttributeValue`
- [x] **Gate: create, list, get-url, and delete a queue via `aws sqs`** — verified
      against the real `aws-cli`, including idempotent re-create, a conflicting
      re-create, prefix filtering, and deleting by the URL `create-queue` returned
- [x] Paging on `ListQueues` — `MaxResults` and `NextToken`, by cursor rather than
      offset, so churn between pages cannot skip or repeat a queue. A token comes back
      whenever more remain, even unasked, rather than truncating at the 1000 cap
      silently. Verified with botocore's own paginator (`--page-size`, `--max-items`,
      `--starting-token`).

## ✅ M3 — The produce/consume loop

- [x] `enqueue`, `claim_next`, `ack` in the engine and memory store, including
      priority ordering, per-queue delay, and a new receipt handle on each redelivery
- [x] `SendMessage`, `ReceiveMessage`, `DeleteMessage`, including per-message
      `DelaySeconds` and a per-receive `VisibilityTimeout`
- [x] `MD5OfMessageBody` in send/receive responses — some SDKs verify this checksum
      and will error out if it is absent or wrong. Note the field is `MD5OfMessageBody`
      on send but `MD5OfBody` on receive; botocore checked both against a live server
- [x] Opaque receipt handles, and `ReceiptHandleIsInvalid` on a stale one
- [x] `MaxNumberOfMessages`, and the SQS response-shape rules for an empty receive
      (`Messages` omitted, not an empty array)
- [x] **Gate: send a message, receive it, delete it, confirm it does not come back** —
      verified against the real `aws-cli`
- [x] Message system attributes on receive. Both `AttributeNames` (deprecated) and
      `MessageSystemAttributeNames` are honoured, and a request carrying both gets the
      union. `All` and the Query protocol's `.*` return every attribute there is:
      `SentTimestamp`, `ApproximateReceiveCount`, `ApproximateFirstReceiveTimestamp`.
      An attribute named explicitly that NexQ has no value for — `SenderId`, or a FIFO
      attribute — is refused with `InvalidAttributeName`, while `All` stays silent about
      it, since `All` means "whatever you have". Verified against the real `aws-cli`,
      which does not filter the names client-side
- [x] `MessageAttributes` on send and receive, with their own MD5. `String`, `Number`,
      and `Binary`, custom labels included, validated against SQS's own published rules
      and counted towards the message size limit. On receive, selected with
      `MessageAttributeNames` — `All`, `.*`, exact names, or a `bar.*` prefix — and a
      name the message does not carry is a miss rather than an error, since the name is
      the producer's to choose.

      `MD5OfMessageAttributes` is checked against four digests **AWS itself published**
      in the `aws-cli` documentation examples, whose outputs are real SQS responses. That
      also settled a question the spec text does not: on receive the digest covers the
      attributes *returned*, not every attribute the message holds — AWS's own example
      shows a different digest for the same message when only one of its two attributes
      was requested. Verified end to end against the real `aws-cli`, with the digest
      cross-checked against an independent implementation of the encoding.

## ✅ M4 — Long-polling receive

The first piece of NexQ's own design to land, and the reason the primary holds
in-process waiters instead of polling a backend.

- [x] In-process waiter registry per queue, woken by the enqueue path —
      `nexq-core::waiters`, one `Notify` per queue that has actually been waited on, and
      the entry is dropped with the queue. A waiter is armed _before_ it looks at the
      queue, which is what makes a lost wake impossible: anything enqueued from that
      moment either finds the waiter registered or is seen by the look itself
- [x] `WaitTimeSeconds` honored, returning empty on timeout. Omitting it falls back to
      the queue's `ReceiveMessageWaitTimeSeconds`, which had been stored since M2 and
      never used, so a queue can now make long polling its consumers' default. Capped at
      20 seconds in the engine as well as the facade, so config cannot get around the
      protocol's limit
- [x] Wake ordering re-evaluated at wake time rather than fixed at registration — a
      wake carries no payload, so a woken consumer re-runs the claim and gets whatever
      ranks first _then_. Pinned by a test where the enqueue that causes the wake is a
      delayed message: the waiter must find nothing and keep waiting rather than be
      handed the message that woke it
- [x] The wait applies to the first message only. Asking for ten when three exist
      returns three rather than holding the request open for seven more, which is SQS's
      behaviour and the useful one
- [x] Shutdown releases waiters instead of waiting for them. Long polls are in-flight
      requests, so graceful shutdown took **19.7 seconds** with one outstanding; the
      engine is now told to stop waiting when shutdown starts, and it takes **0.01
      seconds**. Waiters get their normal empty answer rather than a dropped connection
- [x] Deleting a queue releases the consumers waiting on it, rather than leaving them
      parked on a queue that no longer exists
- [x] **Gate: `receive-message --wait-time-seconds 20` blocks, then returns
      immediately when a message is sent from another terminal** — verified against the
      real `aws-cli`: returned 4.03s after the receive began, on a send 3s in, rather
      than waiting out the 20s

What does _not_ wake a waiter yet, and is M5's timer rather than an event: a delay
elapsing, and a visibility timeout lapsing. Both make a message claimable without an
enqueue, so a consumer only learns about them on its next receive.

## ✅ M5 — Visibility timeout and redelivery

Mostly landed early, since a claim that cannot expire would hand the same message to
two consumers. Expiry is noticed when someone next tries to claim, which is where it
belongs — see Future for why that is not a timer.

- [x] Visibility timeout on claim, honoured on the next claim attempt
- [x] Redelivery on expiry, under a new receipt handle that invalidates the old one
- [x] `VisibilityTimeout` as a queue attribute and a per-receive override
- [x] **Gate: receive without deleting, wait out the timeout, receive it again** —
      verified against the real `aws-cli` with `--visibility-timeout 1`
- [x] `ApproximateReceiveCount` surfaced to clients, counting the delivery in progress
      so a first receive reports `1`. `ApproximateFirstReceiveTimestamp` came with it,
      and stays pinned to the first delivery rather than the latest
- [x] `ChangeMessageVisibility`, counted from now rather than from the receive, so the
      one call both extends a claim that needs longer and shortens one that does not.
      The handle stays valid, since this changes when a claim ends and not whose it is.

      `VisibilityTimeout: 0` hands the message straight back, which is the useful edge —
      a consumer that cannot do the work returns it instead of holding it until the claim
      lapses. That is a client action making a message claimable, so it **wakes a waiting
      consumer** exactly as an enqueue does; a non-zero timeout deliberately does not,
      since the message stays invisible and waking anyone would just send them to the
      store for nothing. Verified against the real `aws-cli`: a long poller got a
      handed-back message in 3.8s rather than timing out at 20.

      `MessageNotInflight` is not distinguished from `ReceiptHandleIsInvalid` — a receipt
      handle only exists while a claim does, so the two are the same condition here.

## M6 — The rest of what the CLI commonly exercises

- [ ] `GetQueueAttributes` / `SetQueueAttributes`, including the approximate-count
      attributes
- [ ] `PurgeQueue`
- [ ] `SendMessageBatch`, `DeleteMessageBatch`, `ChangeMessageVisibilityBatch`
- [ ] Message attributes, with their own MD5
- [ ] `DelaySeconds` on send and as a queue attribute
- [ ] Audit which operations `aws-cli` and the SDKs actually reach for, and stop
      there — replicating the full API surface is an explicit non-goal

## M7 — Lock it in

- [ ] Acceptance test that drives a real `aws-cli` against a running NexQ, scripted
      rather than manual
- [ ] Run it in CI, with `aws-cli` available to the job
- [ ] Repeat the run against at least one AWS SDK in another language, since SDK
      behavior differs from the CLI's in checksum validation and retries
- [ ] `README.md`: the `aws configure` + `--endpoint-url` setup, end to end

# M8 - SSL

- [ ] Add SSL support to servers

# Future

- FIFO Queues
- AWS Query/XML protocol
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

  Also deliberately **not a timer**, which is what this item used to say. A timer would
  have to fire at every `claim_expires_at`, and in the happy path the consumer acks long
  before that — so almost every one of those timers would wake a consumer that finds
  nothing. Its cost would scale with _messages_ while its benefit scales with _waiting
  consumers on idle queues_, which is backwards. The cheap version is one
  `Store::next_visible_at` query per long poll that finds the queue empty, sleeping
  `min(deadline, next visible)`: nothing on the hot path, O(1) per waiting consumer
  rather than per message, and it needs one new `Store` method — `MIN(visible_at)`,
  indexable in SQL — plus a conformance case.

- `DelaySeconds` precision on a low-volume queue. The feature works, but a delayed
  message is picked up when a waiting consumer next looks rather than when its delay
  elapses, per the item above. Worth watching rather than ignoring: delayed retry with
  backoff is a common pattern, and it is the one case where somebody would actually
  notice work landing seconds late. Fixed by the same change; needs nothing of its own.

---

## After this milestone

Rough order, not yet committed — revisit once M1–M7 are done and the facade boundary
has been exercised for real:

1. REST facade with the extended feature set, plus the OpenAPI spec and generated
   clients
2. Priority, position-in-queue, and DLQ + redrive in the core engine
3. Durable backends: SQLite, then Postgres, against the conformance suite
4. The `nexq` CLI over the generated client
5. Prometheus metrics; Dockerfile and single-node Helm chart
6. Multi-node HA: lease election, transparent proxying, rehydration on failover
7. OpenSearch/Elasticsearch backends
8. Web UI, then the KEDA external scaler

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
- **Web UI: Angular with [Optimus UI](https://optimus.openng.org/installation)**
  (`ng add @openng/optimus-ui`), which needs Angular v21+ (v22 for Optimus v2) and
  RxJS 7.8.1+. Its component set — forms, tables, panels, charts — covers what the
  queue/cluster inspection and DLQ management views need, so the UI work is layout
  and API wiring rather than component building. Does not affect the backend: the SPA
  is generated from the OpenAPI spec and served as embedded static assets.

## Decisions still open

- Nothing blocking. Next real fork is per-principal authorization (below), and it is
  not needed for any milestone through M7.
