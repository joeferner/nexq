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

## M1 — A signed request gets a valid response

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
- [ ] Reject stale signatures with a clock-skew window. The timestamp is used in the
      signature but never checked for freshness, so **a captured request replays
      forever**. Wants a configurable tolerance, since air-gapped clocks drift.
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

## M2 — Queue lifecycle against the memory backend

- [x] Domain model: queue, message, receipt handle — `QueueName` validates on
      construction, `Message` and `ClaimedMessage` separate the durable item from a
      time-limited claim
- [x] `Store` trait, behind `dyn` via `async-trait`, covering queue lifecycle only —
      create, get, delete, list — plus a `StoreError` that separates "no such queue"
      from "the backend broke"
- [x] Memory store in `nexq-store-memory`, its own crate alongside the other backends
- [ ] `nexq-store-conformance` suite covering the operations implemented so far,
      running green against the memory store
- [x] Core engine operations: create, get, delete, list — with idempotent creation
      decided here, so every facade inherits it
- [x] Queue URL construction *and parsing* from the configured public base URL — the
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
- [ ] Paging: `MaxResults`/`NextToken` on `ListQueues` are accepted and ignored, so
      every queue comes back in one response

## M3 — The produce/consume loop

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
- [ ] Message system attributes on receive: `AttributeNames`/
      `MessageSystemAttributeNames` are ignored, so `ApproximateReceiveCount` and
      `SentTimestamp` are never returned even when asked for
- [ ] `MessageAttributes` on send and receive, with their own MD5. Currently refused
      rather than silently dropped, so no data is lost — but a client that needs them
      cannot use this facade

## M4 — Long-polling receive

The first piece of NexQ's own design to land, and the reason the primary holds
in-process waiters instead of polling a backend.

- [ ] In-process waiter registry per queue, woken by the enqueue path
- [ ] `WaitTimeSeconds` honored, returning empty on timeout
- [ ] Wake ordering re-evaluated at wake time rather than fixed at registration
- [ ] **Gate: `receive-message --wait-time-seconds 20` blocks, then returns
      immediately when a message is sent from another terminal**

## M5 — Visibility timeout and redelivery

Mostly landed early, since a claim that cannot expire would hand the same message to
two consumers — the part still missing is the timer, not the expiry.

- [x] Visibility timeout on claim, honoured on the next claim attempt
- [x] Redelivery on expiry, under a new receipt handle that invalidates the old one
- [x] `VisibilityTimeout` as a queue attribute and a per-receive override
- [x] **Gate: receive without deleting, wait out the timeout, receive it again** —
      verified against the real `aws-cli` with `--visibility-timeout 1`
- [ ] An in-process expiry timer. Expiry is currently noticed only when someone next
      tries to claim, which is enough for correctness but cannot wake a long-poller
      waiting on a message whose claim just lapsed
- [ ] `ApproximateReceiveCount` surfaced to clients — it is counted, but receive does
      not return message attributes yet
- [ ] `ChangeMessageVisibility`

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
- [ ] Add SSL to clients

# Future

- FIFO Queues
- AWS Query/XML protocol
- Per-principal authorization — the registry authenticates *who* a caller is, but
  every authenticated principal can do everything. Rules like "this consumer may
  receive from `jobs` but not purge it" need a permissions model, and are the reason
  per-principal keys are the recommended default now rather than later.

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
  yet act on *who* a principal is — see the authorization item under Future.
- **Web UI: Angular with [Optimus UI](https://optimus.openng.org/installation)**
  (`ng add @openng/optimus-ui`), which needs Angular v21+ (v22 for Optimus v2) and
  RxJS 7.8.1+. Its component set — forms, tables, panels, charts — covers what the
  queue/cluster inspection and DLQ management views need, so the UI work is layout
  and API wiring rather than component building. Does not affect the backend: the SPA
  is generated from the OpenAPI spec and served as embedded static assets.

## Decisions still open

- Nothing blocking. Next real fork is per-principal authorization (below), and it is
  not needed for any milestone through M7.
