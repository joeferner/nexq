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

## ✅ M6 — The rest of what the CLI commonly exercises

- [x] `GetQueueAttributes` / `SetQueueAttributes`, including the approximate-count
      attributes. Ten reported under `All`: the three settable ones, the three counts,
      `CreatedTimestamp`, `LastModifiedTimestamp`, `MaximumMessageSize`, and `QueueArn`.

      `SetQueueAttributes` is a genuine partial update — naming `VisibilityTimeout` does
      not reset `DelaySeconds` — and is all-or-nothing, so a request mixing a good
      attribute with a bad one changes neither. Read-only attributes are refused rather
      than ignored.

      Timestamps are epoch **seconds** here, where a message's are milliseconds; the same
      codebase now does both, so there is a test naming the distinction. `Queue` gained
      `last_modified_at`, since reporting the creation time for both would have been a
      plausible-looking lie.

      Attributes NexQ knows but cannot answer — `MessageRetentionPeriod`, `RedrivePolicy`,
      the FIFO four, the SSE three, `Policy` — are refused with `InvalidAttributeValue`
      and a reason, rather than `InvalidAttributeName`, since the client has not made a
      spelling mistake. `All` omits them silently, as with message system attributes.

      New config: `aws_api.region`, default `us-east-1`, used *only* for the region slot
      in a queue ARN. It is not checked against what a client signs with — any region
      still works. Verified against the real `aws-cli`.

- [x] `PurgeQueue` — empties a queue and keeps it, taking **in-flight messages with
      it**, so a consumer holding a receipt handle across a purge finds it invalid. A
      backend that only deleted what was visible would leave claimed messages to reappear
      when their claims lapsed, which is a purge that quietly did not purge; the
      conformance case for it fails against exactly that mutation.

      No rate limit, unlike SQS, which refuses a second purge within sixty seconds with
      `PurgeQueueInProgress`. That error covers SQS's purge being asynchronous; this one
      has finished when it answers, so refusing would be a limitation invented for its own
      sake. Verified: three purges in a row all succeed.

      The only operation logged at `info` rather than `debug`, since it is the one that
      destroys data irrecoverably — the line carries how many messages went.

- [x] `SendMessageBatch`, `DeleteMessageBatch`, `ChangeMessageVisibilityBatch`, sharing
      a `batch` module for entry parsing and result rendering. **Partial success is the
      point**: each entry succeeds or fails on its own, and a batch with one bad entry
      answers `200` with both `Successful` and `Failed` lists. Verified against the real
      `aws-cli`.

      Each batch entry runs the *same code* a lone request does — `send_one`,
      `delete_one`, `change_one` — rather than a second implementation that could drift.
      A test asserts a batched send and a single send produce the same
      `MD5OfMessageAttributes`.

      Nothing reaches the engine: batching decomposes into operations it already has.
      One storage round trip per batch is an optimisation for a backend that can do it,
      not a change in meaning.

      Five failures reject the whole batch, all about the *list* rather than its
      contents: `EmptyBatchRequest`, `TooManyEntriesInBatchRequest`,
      `BatchEntryIdsNotDistinct`, `InvalidBatchEntryId`, `BatchRequestTooLong`. A missing
      queue joins them — the `QueueUrl` belongs to the request, so ten copies of
      `QueueDoesNotExist` in a `Failed` list would be worse than one raised error, and
      SDKs raise on the latter while the former can be silently ignored.

      `SenderFault` is read off the error's status rather than tracked separately: a 4xx
      *means* the caller was wrong. `VisibilityTimeout` is optional per entry, as SQS's
      model marks it, falling back to the queue's configured timeout.

- [x] All 23 SQS operations are now recognised, not just the 14 implemented — so
      `TagQueue` answers `NotImplemented` rather than `UnknownOperationException`, which
      would send a client looking for a typo it has not made
- [x] Audit which operations `aws-cli` and the SDKs actually reach for, and stop
      there — replicating the full API surface is an explicit non-goal.

      Done empirically: every one of SQS's 23 operations has an `aws sqs` subcommand, and
      all 23 were driven against a running NexQ with well-formed arguments. The 14
      implemented ones **all work**; the other 9 all answer `NotImplemented` cleanly, so
      the CLI reports something a human can act on rather than a parse failure.

      The finding that mattered was a negative one: 28 requests arrived, 23 distinct
      operations, and **exactly the ones asked for**. The CLI makes no implicit calls — no
      `GetQueueAttributes` behind a receive, no `ListQueues` behind a lookup — so nothing
      already working secretly depends on an operation that is not built. That is what
      makes stopping here safe rather than merely convenient.

      Verdict on the remaining 9, by what each would actually need:

      - **Access policies** (`AddPermission`, `RemovePermission`, and the `Policy`
        attribute) — **not planned**. Implementing them means implementing an
        IAM-policy evaluator, a second authorization model beside NexQ's own credential
        registry. Two models deciding the same question is worse than one, so this is a
        deliberate no rather than a not-yet.
      - **DLQ and redrive** (`ListDeadLetterSourceQueues`, `StartMessageMoveTask`,
        `CancelMessageMoveTask`, `ListMessageMoveTasks`) — deferred **with DLQ itself**,
        which is already on the list after this milestone. They are the API around a
        feature, not features of their own, and building them first would mean four
        operations reporting on something that does not exist.
      - **Tagging** (`TagQueue`, `UntagQueue`, `ListQueueTags`) — the only honest
        maybe, moved to Future. It needs nothing of the engine, just a string map on a
        queue, and NexQ would attach no meaning to it. Worth building if
        infrastructure-as-code support becomes a goal, since a Terraform
        `aws_sqs_queue` sets tags; worth nothing otherwise.

## ✅ M7 — Lock it in

- [x] Acceptance test that drives a real `aws-cli` against a running NexQ, scripted
      rather than manual — `cargo xtask acceptance-cli`, or `make acceptance-cli`. Twelve checks
      covering the queue lifecycle, paging through botocore's own paginator, the
      produce/consume loop, message and system attributes, long polling, visibility
      changes, batches with a partial failure, queue attributes and counts, purge,
      the authentication failures, and `NotImplemented` on a real operation we do not
      have.

      In `xtask` rather than a shell script so it runs the same way on a laptop as in CI
      — a script only CI runs is a script that rots. It builds the server itself, finds
      a free port, waits for the port to answer rather than sleeping, and reports every
      failing check rather than stopping at the first.

      Deliberately **not** part of `make pre-commit`: it takes about a minute of wall
      clock, most of it the CLI's own startup cost across ~50 invocations plus the real
      long-poll waits it has to sit through.

      Checked that it *fails* as well as passes, by breaking three things in turn: a
      wrong body MD5 (caught by "send, receive, delete"), a long poll that returns at
      once (caught by "long polling"), and SigV4 accepting any signature (caught by
      "authentication"). A green acceptance suite that cannot go red is decoration.

- [x] Run it in CI, with `aws-cli` available to the job — its own `acceptance` job.
      Nothing to install: GitHub's ubuntu runners ship AWS CLI 2.36.24, checked against
      their published runner manifest rather than assumed, and near-identical to the
      2.36.30 used locally. **No secrets and no service containers**, since NexQ is its
      own trust root and the memory backend needs nothing — so it runs on a pull request
      from a fork exactly as it does locally. The job also prints `aws --version`, so a
      runner image dropping the CLI fails loudly instead of mysteriously.
- [x] Repeat the run against at least one AWS SDK in another language, since SDK
      behavior differs from the CLI's in checksum validation and retries — the AWS SDK
      for JavaScript, in `acceptance/node/`, run by `cargo xtask acceptance-node` or
      `make acceptance-node`. Node rather than Python because the CLI _is_ botocore, so a
      Python SDK would have re-tested the same implementation.

      That worry turned out to be exactly right. **This SDK validates the MD5s and
      botocore does not**: `SendMessage`, `SendMessageBatch`, and `ReceiveMessage` each
      recompute the body digest in middleware the client installs by default, and throw
      rather than return. Confirmed by reading the middleware wiring, then by breaking
      NexQ's digest and watching it refuse the message with
      `Invalid MD5 checksum on messages: <ids>`. Until this suite existed, nothing had
      held NexQ's checksums to a client that checks them — the CLI suite only caught a
      wrong digest because it asserts the literal value.

      Second finding: the SDK deserialises errors into **typed classes** picked from the
      `__type` field, so `instanceof QueueDoesNotExist` is a check on the error envelope
      by something not written against us. Breaking a code turned it into a generic
      `SQSServiceException`, which the suite caught. Breaking the *namespace* did not —
      the SDK matches only the part after the `#`, so `com.amazonaws.sqs` is decorative
      to this client. Worth knowing rather than assuming.

      Seven checks, chosen for where the two clients differ rather than to repeat the CLI
      suite: the round trip and the batch under MD5 validation, typed errors, long
      polling against the SDK's own timeouts, its paginator, message attributes with
      binary sent as bytes rather than pre-encoded base64, and queue attributes with a
      visibility hand-back. About nine seconds including the install, from a committed
      lockfile so a run is reproducible. Its own CI job.

- [x] `README.md`: the `aws configure` + `--endpoint-url` setup, end to end. The root
      README was twenty lines of feature tables with no way in — the setup already
      existed, but only in the AWS facade's README, which is not where anyone lands.

      Split so the two cannot drift into disagreeing: the root has the shortest path that
      works — `make server`, four exports, create/send/receive with the real output — plus
      configuration, what works today, and the development commands. The facade README
      keeps the fuller version, and now says so at the top: the alternatives to those
      exports, `aws configure`, `~/.aws/config`, running behind a proxy, and the error
      table. Every command in the quick start was run as written, and every link checked.

## ✅ M8 - SSL

- [x] Add SSL support to servers — `[aws_api.tls]` with `certificate`, `private_key`, and
      an optional `client_ca` for mutual TLS, plus `[client_tls]` for the other direction.

      TLS is a different listener and nothing else, via `tls-listener`, which performs
      handshakes **off the accept path**. The obvious alternative — implementing axum's
      `Listener` and handshaking inside `accept()` — would have let one client open a
      connection, say nothing, and stop the server accepting anyone else. Graceful
      shutdown and the long-poll draining are untouched as a result.

      `ring` rather than the default `aws-lc-rs` provider, since it needs only a C
      compiler where aws-lc-rs also wants cmake. The provider is named explicitly rather
      than inferred, because inference works only while exactly one is compiled in and a
      future dependency enabling a second would turn that into a panic on first
      connection.

      Certificates load at **startup**: a wrong path, an empty file, two keys in one file,
      or a key that does not match its certificate stops the server coming up rather than
      surfacing as a client reporting "handshake failed". The loader lives in
      `nexq-core::tls` so REST gets the same one, and its error messages name the file and
      the setting.

      `[client_tls]` is config with **no consumer yet**, and deliberately so — it is for
      `nexq-client`, the CLI, and later TLS to SQL and search backends. Loaded and
      validated at startup regardless, and its loader is tested, so the plumbing is known
      good before anything leans on it.

      Two things worth having found:

      - `openssl req -x509` produces a certificate marked `CA:TRUE`, and rustls refuses to
        use a CA certificate as an end entity. The first test failed with
        `CaUsedAsEndEntity`; the fix was a proper authority-plus-leaf chain, which is the
        realistic shape anyway and exercises chain handling.
      - Breaking the mTLS gate on purpose made its test **hang** rather than fail — a
        served keep-alive connection left the read waiting for a close that never came. It
        now sends `Connection: close` under a timeout, and the same mutation fails in
        under a second.

      Covered by 8 loader tests, 6 server tests including a real handshake, a real mTLS
      refusal-and-admission, and a plain-HTTP client getting nothing from a TLS port —
      plus a TLS check in the CLI acceptance suite where the CLI is told to *trust the
      authority* rather than skip verification, so the chain has to genuinely check out.

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
- Message expiry, and the `MessageRetentionPeriod` attribute that reports it. NexQ keeps
  a message until someone deletes it, so there is no retention period to report and
  `GetQueueAttributes` refuses the attribute with that as the stated reason. Answering
  with SQS's four-day default instead would promise an expiry that never comes, and a
  client relying on it would find its queue growing without limit.

  A real implementation is a sweep rather than a per-message timer, for the same reason
  the visibility item above is not a timer: the cost should scale with queues, not with
  messages. It also raises a question NexQ has not had to answer yet — whether an expired
  message is dropped or sent to a dead-letter queue — so it is worth doing alongside DLQ
  rather than before it.

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
