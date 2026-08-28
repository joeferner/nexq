# REST API

NexQ's own API, and the complete one. Per the plan's Q12, the core engine's operation set
is the single source of truth: the [SQS/SNS facade](../nexq-api-aws/README.md) is a
compatibility-only translation of a *subset* of it, while this is a native mapping onto
**all** of it, extensions included. No capability should ever exist only behind SQS and be
unreachable from here.

It is also the contract every client is generated from — the CLI, the web UI, and any
published SDK — so the routes and types in this crate are the one definition of the API,
and [`openapi.json`](openapi.json) is generated from them rather than written.

> **Under construction.** Queues are a complete resource — create, read, list with paging,
> delete — and messages are not: `receive` is the only message operation, so nothing here
> can send or delete one yet. The plumbing around it all is real: listener, TLS,
> authentication, one error envelope, long polling, a generated and committed spec, and
> browsable documentation. See [`todo.md`](../../todo.md) M9 for the order the rest arrives
> in. Until sending lands, the SQS facade is the one that can drive a queue end to end.

# Features

:scroll: - Future
:ballot_box_with_check: - Partially Complete
:white_check_mark: - Completed

## The contract

- :white_check_mark: OpenAPI 3.1, generated from the routing table by
  [`aide`](https://docs.rs/aide) — `api_route` records the operation while registering the
  handler, so the document cannot describe an endpoint that does not exist or miss one that
  does. `utoipa` was considered and not chosen (Q18a): its per-handler attribute amounts to
  writing the OpenAPI structure by hand
- :white_check_mark: Field descriptions, types, and bounds derived from the Rust types by
  `schemars` — the doc comments on a field *are* its documentation in the spec
- :white_check_mark: [`openapi.json`](openapi.json) is **committed**, and a test fails when
  it and the code disagree. Without it a change to the published API is invisible in review,
  and every generated client changes with it silently. `make openapi` regenerates;
  `make openapi-check` verifies, and runs in `make pre-commit` and CI
- :white_check_mark: Served at `/api/v1/openapi.json`, byte-identical to the committed file
  — so `curl … | diff - crates/nexq-api-rest/openapi.json` is a real check
- :white_check_mark: A browsable [Scalar](https://github.com/scalar/scalar) page at
  `/api/v1/docs`, served from the binary with no CDN, so it works air-gapped
- :scroll: Generated clients: `nexq-client` (the Rust SDK the CLI is built on) and the web
  UI's own client, from this one document

## Authentication

- :white_check_mark: Bearer token — `<key_id>.<secret>` for a credential in the same
  registry the SQS facade verifies signatures against, so there is one set of credentials to
  issue and store. Not SigV4: that exists to satisfy unmodified AWS SDKs, and nothing here
  is one (Q10b)
- :white_check_mark: A layer over every route rather than a per-handler check, so a new
  route cannot accidentally be an unauthenticated one
- :white_check_mark: One `401` for an unknown key id and for a wrong secret alike, so a
  caller cannot enumerate which key ids exist — unlike the SQS facade, which has to
  distinguish them because AWS clients report on the difference. `WWW-Authenticate: Bearer`
  accompanies it
- :white_check_mark: The secret is compared without returning early on the first differing
  byte. Its limits are written down where it lives: the length is still compared first, and
  this is not a defence against a determined local attacker
- :white_check_mark: HTTPS via `[rest_api.tls]`, on the same loader the SQS facade uses, so
  a bad certificate stops the server coming up rather than surfacing as "handshake failed".
  Handshakes happen off the accept path
- :white_check_mark: Mutual TLS via `client_ca` — a transport gate on top of the token, not
  a replacement for it
- :scroll: Per-principal authorization. Every authenticated principal may do everything

## Queue operations

A queue is addressed by **name in the path** — `/api/v1/queues/jobs` — not by a URL the
server hands out and the client must send back.

- :white_check_mark: `PUT /queues/{queue}` — create, or confirm one exists. Idempotent when
  the attributes match and a `409` when they differ, so a live queue is never reconfigured
  by accident. `PUT` rather than `POST` to a collection precisely because that is what
  idempotent-and-addressed-by-name means
- :white_check_mark: `GET /queues/{queue}` — the queue, its timestamps, and its attributes
- :white_check_mark: `DELETE /queues/{queue}` — `204`, and `404` when there was nothing to
  delete. Takes the queue's messages with it and releases consumers waiting on it
- :white_check_mark: `GET /queues` — one page at a time, filtered by `prefix`, with
  **cursor** paging rather than offsets: a queue created or deleted between pages cannot
  make a caller skip one or see it twice
- :ballot_box_with_check: Attributes on create — `visibility_timeout_seconds`,
  `delay_seconds`, `receive_wait_time_seconds`. An out-of-range value is refused rather than
  clamped, and an unrecognised one refused rather than dropped
- :scroll: Changing attributes after creation
- :scroll: The message counts
- :scroll: Purge

## Message operations

- :ballot_box_with_check: **Receive** — `POST /api/v1/queues/{queue}/messages/receive`, with
  `max_messages`, `visibility_timeout_seconds`, and `wait_time_seconds`. Returns the
  message id, receipt handle, body, priority, delivery count, and how long the claim has
  left. Message attributes and absolute timestamps are not carried yet — see
  [What receive leaves out](#what-receive-leaves-out)
- :white_check_mark: Long polling, on the **same** in-process waiter registry the SQS facade
  uses — one mechanism with two protocol faces, not two implementations. A send through
  either facade wakes a consumer waiting on the other
- :white_check_mark: Shutdown releases a waiting consumer with its ordinary empty answer
  rather than holding the shutdown open or dropping the connection
- :scroll: Send, delete, and change visibility
- :scroll: Batches

## The extensions this facade exists for

None of these are reachable from any AWS facade, which is why they are here rather than
there. All are M10 rather than M9:

- :ballot_box_with_check: **Per-message priority.** Reported on receive, and there is no way
  to *set* it yet, since there is no send endpoint — so everything currently carries the
  default. The engine and the memory store have ordered by priority since M3
- :scroll: **Position in queue** — "where am I in line", approximate by nature
- :scroll: **Dead-letter queues and redrive**, including the four SQS operations deferred
  along with them
- :scroll: Cluster and leader status, which needs a cluster first

## Not planned

- SigV4 on this facade. It exists for AWS SDK compatibility, and a client written against
  NexQ's own API has no reason to implement a signing procedure
- The SQS wire format's quirks. An empty receive returns `{"messages": []}` rather than
  omitting the field, and a queue is addressed by **name** in the path rather than by a URL
  the server hands out and the client must send back. Both are AWS artifacts — the second
  exists because SQS has accounts and regions to encode — and repeating them in a native API
  would be copying a workaround

---

# Coverage

Stated plainly, because the surrounding machinery is complete enough to look further along
than the surface is:

| | |
| --- | --- |
| Documented operations | 5 — `putQueue`, `getQueue`, `deleteQueue`, `listQueues`, `receiveMessages` |
| Routes serving documentation | 3 (the spec, the page, and its two assets) |
| Authentication | complete |
| Error envelope | complete, and mapped from every `EngineError` variant |
| Queue resource | complete except changing attributes, counts, and purge |
| Message operations | `receive` only — no send, delete, change-visibility, or batches |

A path this facade does not have answers in its own envelope rather than with an empty
`404`, so a client parsing errors never has to special-case a wrong URL.

---

# Using it

```sh
make server
```

That serves the SQS facade on `:8080` and this one on `:8081`, from
[`nexq.toml`](../../nexq.toml) — seeded from
[`nexq.example.toml`](../../nexq.example.toml) if missing. The credential in it is
`AKIANEXQDEV` / `change-me`, so the bearer token is the two joined by a period:

```sh
export NEXQ=http://localhost:8081
export TOKEN=AKIANEXQDEV.change-me
```

## Queues

```sh
curl -s -X PUT "$NEXQ/api/v1/queues/jobs" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"delay_seconds": 5}'
```

```json
{
  "name": "jobs",
  "created_at": "2026-08-27T00:57:23.116130732Z",
  "last_modified_at": "2026-08-27T00:57:23.116130732Z",
  "attributes": {
    "visibility_timeout_seconds": 30,
    "delay_seconds": 5,
    "receive_wait_time_seconds": 0
  }
}
```

An attribute not named takes its **default**, not zero — the visibility timeout above is 30
because nothing asked for another. Timestamps are RFC 3339, which a generated client turns
into its language's own date type.

`PUT` is idempotent: the same request again returns the same queue. The same *name* with
different attributes is a `409` rather than a silent reconfiguration of a live queue:

```sh
curl -s -X PUT "$NEXQ/api/v1/queues/jobs" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"delay_seconds": 6}'
# {"error":{"code":"queue_already_exists","message":"a queue named jobs exists with different attributes"}}
```

```sh
curl -s "$NEXQ/api/v1/queues/jobs" -H "Authorization: Bearer $TOKEN"
curl -s -X DELETE "$NEXQ/api/v1/queues/jobs" -H "Authorization: Bearer $TOKEN"   # 204
```

Deleting takes the queue's messages with it, including ones a consumer is holding, and
releases anyone long-polling on it. Deleting a queue that was never there is a `404` rather
than a silent success — a caller in that position has a different problem from one whose
delete worked.

### Listing, and why the cursor is not an offset

```sh
curl -s "$NEXQ/api/v1/queues?limit=2" -H "Authorization: Bearer $TOKEN"
# {"queues":[{"name":"alpha",…},{"name":"beta",…}],"next_cursor":"beta"}

curl -s "$NEXQ/api/v1/queues?limit=2&cursor=beta" -H "Authorization: Bearer $TOKEN"
# {"queues":[{"name":"delta",…},{"name":"gamma",…}],"next_cursor":"gamma"}
```

`next_cursor` is `null` on the last page, and `?prefix=` filters by name. The cursor names
**where to resume**, so deleting `alpha` between the two requests above still yields
`delta` next — where an offset of 2 would have skipped it. That is the store's keyset
guarantee carried onto the wire rather than re-derived from it.

A cursor is this server's to issue, so a value it did not issue says so — `invalid_cursor`,
not a complaint about your queue name.

## Receiving a message

There is no send endpoint yet, so this uses the SQS facade to produce and this one to
consume — which is also the clearest demonstration that they are one queue and not two:

```sh
export AWS_ACCESS_KEY_ID=AKIANEXQDEV
export AWS_SECRET_ACCESS_KEY=change-me
export AWS_DEFAULT_REGION=us-east-1
export AWS_ENDPOINT_URL_SQS=http://localhost:8080

aws sqs create-queue --queue-name jobs
aws sqs send-message \
  --queue-url http://localhost:8080/000000000000/jobs \
  --message-body "hello from sqs"
```

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"max_messages": 10}'
```

```json
{
  "messages": [
    {
      "id": "18fb94f9-cdd4-4c5f-a94c-41db40e70641",
      "receipt_handle": "ed2c38cc-c6c1-4121-aeaf-1fde14e314b0",
      "body": "hello from sqs",
      "priority": 0,
      "receive_count": 1,
      "claim_expires_in_seconds": 29
    }
  ]
}
```

The `id` is the same `MessageId` `send-message` returned. There is no way to **delete** the
message yet, so it becomes claimable again when the visibility timeout lapses.

Every field of the body is optional, and no body at all is a plain poll of one message under
the queue's own defaults:

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" -H "Authorization: Bearer $TOKEN"
# {"messages":[]}
```

An empty list is a normal answer, including when a long poll runs out — **not** an error, and
not an omitted field the way SQS omits `Messages`.

`claim_expires_in_seconds` is a duration rather than an expiry timestamp, deliberately: a
consumer needs to know how long it has, and answering with a timestamp would make that
depend on the client's clock agreeing with the server's. It truncates rather than rounds, so
a 30-second timeout reads as `29` a moment later — the conservative direction, since it
never claims more time than there is.

## Waiting for work

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"wait_time_seconds": 20, "max_messages": 10}'
```

The request is held open until a message arrives or the wait runs out, and returns the
moment one is sent — including when it is sent **through the SQS facade**, because both
facades wake consumers through the same registry. Twenty seconds is the maximum, as in SQS.

Omitting `wait_time_seconds` falls back to the queue's own
`ReceiveMessageWaitTimeSeconds`, which is a different thing from sending `0`: zero asks for a
plain poll. The wait applies to the *first* message only, so asking for ten when three exist
returns three rather than holding on for seven more.

Two things do not yet wake a waiting consumer, because they happen when a clock runs out
rather than when a client acts: a `DelaySeconds` delay elapsing, and a visibility timeout
lapsing. Both are noticed on the next receive.

Shutting the server down releases waiting consumers immediately with an empty response.

## What receive leaves out

Two things, both deliberate, both waiting on decisions rather than on work:

- **Message attributes.** Carrying them means deciding how a binary value is represented,
  which belongs with the OpenAPI schemas rather than being invented here first.
- **Absolute timestamps** — `enqueued_at` and friends. A date format needs a date crate, and
  choosing one halfway would churn every generated client. What a consumer actually needs
  from a claim is a duration, which is what it gets.

Both are on the SQS facade already, so nothing is unreachable in the meantime.

---

# The OpenAPI document

```sh
curl -s "$NEXQ/api/v1/openapi.json"
```

Unauthenticated, on purpose: it describes the shape of the API and carries nothing
deployment-specific — no queue names, no data — and a client generator has to be able to
fetch it. It is byte-identical to the committed copy:

```sh
curl -s "$NEXQ/api/v1/openapi.json" | diff - crates/nexq-api-rest/openapi.json
```

Regenerate it after changing a route or a type:

```sh
make openapi         # writes it
make openapi-check   # verifies it, and runs in pre-commit and CI
```

The check exists because the interesting diff is not the Rust one. A renamed field is one
line of code and a change to every client generated from this document; committing the
document puts that in the pull request.

## The documentation page

```
http://localhost:8081/api/v1/docs
```

Scalar, rendering the document above. The bundle is **vendored into this crate** rather than
loaded from a CDN — see [`assets/scalar/PROVENANCE.md`](assets/scalar/PROVENANCE.md) for its
version, licence, and how to refresh it — because a documentation page that renders blank in
an air-gapped deployment is worse than not having one.

Two things about that bundle are worth knowing, since both send data somewhere else by
default and both are switched off:

- It routes its "try it" requests through `https://proxy.scalar.com`, which would hand
  whatever token you paste into the page to a third party.
- It fetches webfonts from `https://fonts.scalar.com`.

The page's `Content-Security-Policy` is the backstop rather than the configuration: with
`connect-src 'self'` and `font-src 'self' data:`, the browser refuses either request even if
a future version of the bundle renames the option that turns them off.

---

# When something is refused

One envelope for everything, with a `code` that is part of the contract and safe to branch
on, and a `message` for a human that may change:

```json
{ "error": { "code": "queue_not_found", "message": "no queue named jobs" } }
```

| Status | `code` | Meaning |
| --- | --- | --- |
| 401 | `unauthorized` | No bearer token, or one that does not check out. Deliberately the same answer for an unknown key id and a wrong secret |
| 404 | `queue_not_found` | No queue by that name |
| 404 | `no_such_route` | No route for that method and path |
| 400 | `invalid_queue_name` | The name in the path is not a legal queue name |
| 400 | `invalid_max_messages` | Outside 1–10. Refused rather than clamped: quietly returning ten when fifty were asked for leaves the caller's misunderstanding intact |
| 400 | `invalid_wait_time` | Over the 20-second protocol maximum |
| 400 | `invalid_request_body` | The JSON does not fit — `serde`'s message names the field and the column |
| 400 | `invalid_query_parameter` | A query parameter is misspelled or malformed, and is named |
| 400 | `invalid_queue_attribute` | An attribute is outside its range, which the message gives |
| 400 | `invalid_limit` | A page limit outside 1–1000 |
| 400 | `invalid_cursor` | A cursor this server did not issue |
| 415 | `unsupported_media_type` | A body was sent as something other than `application/json`. Send no body at all to use the defaults |
| 409 | `queue_already_exists` | A queue of that name exists with different attributes |
| 409 | `conflict` | Concurrent change; the request was fine and retrying should work |
| 413 | `message_too_large` | Over the 256 KiB limit |
| 400 | `invalid_receipt_handle` | The handle was never issued, is spent, or its claim lapsed |
| 500 | `internal_error` | The storage backend failed. The detail is in the server log and deliberately not in the response, since a backend error can name hosts, paths, or credentials |

A typo in a field name is refused rather than ignored, so nobody is told a setting was
applied when it was silently dropped:

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"visibility_timeout": 30}'
```

```json
{"error":{"code":"invalid_request_body","message":"Failed to deserialize the JSON body into the target type: visibility_timeout: unknown field `visibility_timeout`, expected one of `max_messages`, `visibility_timeout_seconds`, `wait_time_seconds` at line 1 column 21"}}
```

Run the server with `RUST_LOG=nexq=debug` to see which principal each request authenticated
as and why anything was rejected.

---

# Serving HTTPS

```toml
[rest_api]
bind_addr = "0.0.0.0:8081"

[rest_api.tls]
certificate = "/etc/nexq/fullchain.pem"
private_key = "/etc/nexq/key.pem"
client_ca   = "/etc/nexq/client-ca.pem"   # optional: require client certificates
```

The table's presence is the switch — there is no `enabled` flag, since that would allow "on,
with no certificate". Both paths are read at **startup**, so a wrong path, an empty file, two
keys in one file, or a key that does not match its certificate stops the server coming up.

TLS is **per facade and not inherited**: switching it on here does not switch it on for the
SQS facade, and the reverse. That is more verbose when both want the same certificate, and it
is what lets the SQS facade stay on a private interface in plain HTTP while this one serves
the network over TLS.

Worth saying plainly: **the bearer token is presented in full on every request.** Unlike
SigV4, where a signature covers one request and the secret never crosses the wire, anyone who
can read the traffic can replay it. That is the trade Q10b makes for not asking clients to
implement a signing procedure, and it is why this section exists.

---

# Its relationship to the SQS facade

Two listeners, one `Engine`, one credential registry. A separate **port** rather than a path
prefix, because the SQS protocol puts its operation in a header and posts everything to `/`:
the two cannot share a listener without one giving up its natural URL shape. Separate ports
also let one be firewalled or TLS-terminated differently from the other.

What that buys is checked in
[`tests/cross_facade.rs`](tests/cross_facade.rs), over real sockets rather than in-process
routers, since two bound listeners is the claim:

- A message sent through SQS is received through REST — asserting the **message id**, not
  just the body, so it is the same message and not a lookalike.
- A message claimed over REST is **invisible to SQS**. Shared storage is a weaker property
  than shared claims: two facades could read one store and still both hand out the same
  message.
- An SQS send **wakes a REST long poll**, which is the strongest available evidence that the
  waiter registry is shared rather than duplicated.
- Neither facade serves the other's routes.

Each is verified to fail when REST is given its own engine.
