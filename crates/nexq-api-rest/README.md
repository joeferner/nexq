# REST API

NexQ's own API, and the complete one. Per the plan's Q12, the core engine's operation set
is the single source of truth: the [SQS/SNS facade](../nexq-api-aws/README.md) is a
compatibility-only translation of a *subset* of it, while this is a native mapping onto
**all** of it, extensions included. No capability should ever exist only behind SQS and be
unreachable from here.

It is also the contract every client is generated from — the CLI, the web UI, and any
published SDK — so the routes and types in this crate are the one definition of the API,
and [`openapi.json`](openapi.json) is generated from them rather than written.

> **Under construction, but usable.** The queue and message surfaces are both complete —
> create, read, list, reconfigure and delete a queue; send, receive, delete, re-time and
> purge messages — so a producer and a consumer can now run entirely against this API.
> What is still missing is the *extended* feature set this facade exists for: position in
> queue, dead-letter queues, and redrive. Per-message priority is here already, since the
> SQS-compatible facade cannot express it at all. See [`todo.md`](../../todo.md) M9 and M10.

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
- :white_check_mark: Every field on the wire is `camelCase`, in request bodies, response
  bodies, and path parameters alike, so a generated JavaScript or Java client needs no
  renaming layer. An unrecognised field is refused rather than ignored, which means a client
  sending the snake_case spelling is told so instead of quietly getting the default. Error
  *codes* stay `snake_case` — a code is a value to match on, not a field name
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
- :white_check_mark: `PATCH /queues/{queue}` — a **partial** update: an attribute it does
  not name keeps its current value, where `PUT` would reset it to a default. All-or-nothing,
  so a request mixing a good attribute with a bad one changes neither
- :white_check_mark: Attributes — `visibilityTimeoutSeconds`, `delaySeconds`,
  `receiveWaitTimeSeconds`. An out-of-range value is refused rather than clamped, and an
  unrecognised one refused rather than dropped
- :white_check_mark: Message counts, via `?counts=true` on either read. Off by default
  because it costs one aggregate **per queue**, so a page of a thousand asks for a thousand
- :scroll: A dead-letter queue and `maxReceiveCount`, which arrive with DLQ itself.
  Refused rather than accepted-and-ignored in the meantime

## Message operations

The collection is `/api/v1/queues/{queue}/messages`; one claim is
`/api/v1/queues/{queue}/messages/{receiptHandle}`.

- :white_check_mark: `POST /messages` — **send**, always a list. Sending one message is a
  list of one, so SQS's `SendMessage`/`SendMessageBatch` pair collapses into a single
  operation. Per-entry results, so one bad message does not sink the rest, identified by
  **position** rather than by ids the client has to invent
- :white_check_mark: `POST /messages/receive` — `maxMessages`,
  `visibilityTimeoutSeconds`, `waitTimeSeconds`. Returns the message id, receipt handle,
  body, priority, delivery count, attributes, and how long the claim has left
- :white_check_mark: `DELETE /messages/{receiptHandle}` — finish with a message. A spent
  handle is refused rather than silently accepted
- :white_check_mark: `PATCH /messages/{receiptHandle}` — re-time a claim, counted from now,
  so the one call both extends a claim and hands a message back with `0`
- :white_check_mark: `POST /messages/delete` and `POST /messages/visibility` — the same, many
  at a time. A `POST` rather than a `DELETE` with a body, which proxies mishandle
- :white_check_mark: `DELETE /messages` — **purge**: emptying the message collection while
  keeping the queue, which is what `DELETE` on a collection means. Takes in-flight messages
  with it, and reports how many went
- :white_check_mark: **Message attributes** on send and receive — `string`, `number`, and
  `binary`, with the producer's own label. Binary travels base64 and is stored as the bytes
  it decodes to, which is what makes the SQS facade's checksum of it come out right
- :white_check_mark: **Per-message priority** on send, which the SQS facade cannot express —
  messages sent through that one all arrive at the default
- :white_check_mark: Long polling, on the **same** in-process waiter registry the SQS facade
  uses — one mechanism with two protocol faces, not two implementations. A send through
  either facade wakes a consumer waiting on the other
- :white_check_mark: Shutdown releases a waiting consumer with its ordinary empty answer
  rather than holding the shutdown open or dropping the connection
- :scroll: Absolute timestamps on a received message — see
  [What receive leaves out](#what-receive-leaves-out)

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
| Documented operations | 12 |
| Routes serving documentation | 3 (the spec, the page, and its two assets) |
| Authentication | complete |
| Error envelope | complete, and mapped from every `EngineError` variant |
| Queue resource | complete, bar the dead-letter settings that arrive with DLQ |
| Message operations | complete: send, receive, delete, re-time, purge, and the multi-entry forms |
| The extensions this exists for | priority only; position and DLQ are M10 |

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
  -d '{"delaySeconds": 5}'
```

```json
{
  "name": "jobs",
  "createdAt": "2026-08-27T00:57:23.116130732Z",
  "lastModifiedAt": "2026-08-27T00:57:23.116130732Z",
  "attributes": {
    "visibilityTimeoutSeconds": 30,
    "delaySeconds": 5,
    "receiveWaitTimeSeconds": 0
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
  -H 'Content-Type: application/json' -d '{"delaySeconds": 6}'
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
# {"queues":[{"name":"alpha",…},{"name":"beta",…}],"nextCursor":"beta"}

curl -s "$NEXQ/api/v1/queues?limit=2&cursor=beta" -H "Authorization: Bearer $TOKEN"
# {"queues":[{"name":"delta",…},{"name":"gamma",…}],"nextCursor":"gamma"}
```

`nextCursor` is `null` on the last page, and `?prefix=` filters by name. The cursor names
**where to resume**, so deleting `alpha` between the two requests above still yields
`delta` next — where an offset of 2 would have skipped it. That is the store's keyset
guarantee carried onto the wire rather than re-derived from it.

A cursor is this server's to issue, so a value it did not issue says so — `invalid_cursor`,
not a complaint about your queue name.

## Sending

Always a list, even for one message — which is why there is no separate batch operation to
choose between:

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"messages": [
        {"body": "urgent work", "priority": 10,
         "attributes": {"City": {"type": "string", "value": "Any City"}}},
        {"body": "ordinary work"}
      ]}'
```

```json
{
  "results": [
    {"index": 0, "status": "accepted", "messageId": "6d3d9901-a845-4564-a4cf-e920283588b7"},
    {"index": 1, "status": "accepted", "messageId": "02cbf29f-3360-47a2-8e8e-a641349a216c"}
  ]
}
```

**A request carrying several messages is not a transaction.** Each is accepted or refused on
its own, and the response reports both — so nine good messages are not lost to one bad one:

```json
{"index": 1, "status": "refused",
 "error": {"code": "invalid_delay", "message": "delaySeconds must be between 0 and 900, got 901"}}
```

That is still a `200`, so **read the results** rather than relying on an error being raised.
Entries are identified by their position in the request, so unlike SQS there are no ids to
invent and no duplicate-id failure to handle. A queue that does not exist is one raised `404`
rather than the same failure repeated in every entry, because the queue belongs to the
request and not to any message in it.

Two whole-request refusals remain: an empty list (`empty_request`) and more than ten
(`too_many_entries`). Ten is the SQS facade's limit too, so a batch that works through one
works through the other.

### Attributes and priority

`priority` is NexQ's own — the SQS-compatible facade cannot express it, so messages sent
through that one all arrive at the default. Higher is served first, which is why the receive
below returns `urgent work` first despite it being sent first only by coincidence.

Attributes are `string`, `number`, or `binary`, with an optional label of the producer's own
(`{"type": "string", "label": "uuid", ...}` is stored the way SQS spells `String.uuid`, so
that facade reports it correctly). A `binary` value travels base64 and is **stored as the
bytes it decodes to** — text that happens to look like base64 and the bytes it decodes to
stay different things, which is what makes the SQS facade's checksum of it come out right.

## Receiving

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"maxMessages": 2}'
```

```json
{
  "messages": [
    {
      "id": "6d3d9901-a845-4564-a4cf-e920283588b7",
      "receiptHandle": "a532dc51-07bc-4c78-9936-56e3ab8cbb1c",
      "body": "urgent work",
      "priority": 10,
      "receiveCount": 1,
      "claimExpiresInSeconds": 29,
      "attributes": {"City": {"type": "string", "value": "Any City"}}
    },
    {
      "id": "02cbf29f-3360-47a2-8e8e-a641349a216c",
      "receiptHandle": "2b275678-25a7-4693-9a20-2572bf9c74f2",
      "body": "ordinary work",
      "priority": 0,
      "receiveCount": 1,
      "claimExpiresInSeconds": 29,
      "attributes": {}
    }
  ]
}
```

## Finishing, or handing back

A receipt handle identifies **one claim**, not the message: a redelivery comes with a new
one. Deleting is how a consumer says it is done — until then the message comes back when its
claim lapses, which is what makes delivery at-least-once.

```sh
# Done with it.
curl -s -X DELETE "$NEXQ/api/v1/queues/jobs/messages/$HANDLE" -H "Authorization: Bearer $TOKEN"

# Need longer: extend the claim, counted from now.
curl -s -X PATCH "$NEXQ/api/v1/queues/jobs/messages/$HANDLE" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"visibilityTimeoutSeconds": 300}'

# Cannot do it: hand it straight back for someone else.
curl -s -X PATCH "$NEXQ/api/v1/queues/jobs/messages/$HANDLE" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"visibilityTimeoutSeconds": 0}'
```

Handing a message back with `0` makes it claimable at once **and wakes a consumer that is
long-polling for one**, so work one consumer could not do reaches the next without waiting
out a timeout. The handle stays usable until someone else claims the message and is given a
new one — re-timing a claim changes when it ends, not whose it is.

Several at a time, with the same per-entry results as a send:

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/delete" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"receiptHandles": ["…", "…"]}'

curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/visibility" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"changes": [{"receiptHandle": "…", "visibilityTimeoutSeconds": 300}]}'
```

A `POST` rather than a `DELETE` carrying a body: a body on `DELETE` is legal and widely
mishandled by proxies and clients.

## Purging

```sh
curl -s -X DELETE "$NEXQ/api/v1/queues/jobs/messages" -H "Authorization: Bearer $TOKEN"
# {"purged":2}
```

`DELETE` on the *message collection*, where deleting the queue itself is `DELETE` one level
up. **Irreversible, and it takes in-flight messages with it** — a consumer working on a
message right now finds its handle refused, because the message is gone. Sparing claimed
messages would be a purge that quietly did not purge, since they would reappear when their
claims lapsed.

Unlike SQS there is no sixty-second cooldown: SQS needs one because its purge is
asynchronous, and this one has finished by the time it answers.

Every field of the body is optional, and no body at all is a plain poll of one message under
the queue's own defaults:

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" -H "Authorization: Bearer $TOKEN"
# {"messages":[]}
```

An empty list is a normal answer, including when a long poll runs out — **not** an error, and
not an omitted field the way SQS omits `Messages`.

`claimExpiresInSeconds` is a duration rather than an expiry timestamp, deliberately: a
consumer needs to know how long it has, and answering with a timestamp would make that
depend on the client's clock agreeing with the server's. It truncates rather than rounds, so
a 30-second timeout reads as `29` a moment later — the conservative direction, since it
never claims more time than there is.

## Waiting for work

```sh
curl -s -X POST "$NEXQ/api/v1/queues/jobs/messages/receive" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"waitTimeSeconds": 20, "maxMessages": 10}'
```

The request is held open until a message arrives or the wait runs out, and returns the
moment one is sent — including when it is sent **through the SQS facade**, because both
facades wake consumers through the same registry. Twenty seconds is the maximum, as in SQS.

Omitting `waitTimeSeconds` falls back to the queue's own
`ReceiveMessageWaitTimeSeconds`, which is a different thing from sending `0`: zero asks for a
plain poll. The wait applies to the *first* message only, so asking for ten when three exist
returns three rather than holding on for seven more.

Two things do not yet wake a waiting consumer, because they happen when a clock runs out
rather than when a client acts: a `DelaySeconds` delay elapsing, and a visibility timeout
lapsing. Both are noticed on the next receive.

Shutting the server down releases waiting consumers immediately with an empty response.

## What receive leaves out

One thing: **absolute timestamps** — `enqueued_at`, and when a message was first delivered.
What a consumer actually needs from a claim is how long it has, which is a duration and does
not depend on the client's clock agreeing with the server's. The format question is settled
now that queue timestamps are RFC 3339, so this is a small addition rather than a decision.

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
| 400 | `empty_update` | A `PATCH` naming no attribute to change |
| 400 | `empty_request` | A list with no entries |
| 400 | `too_many_entries` | More than ten entries in one request |
| 400 | `invalid_delay` | A per-message delay outside 0–900 |
| 400 | `invalid_visibility_timeout` | A visibility timeout outside 0–43200 |
| 400 | `invalid_message_attribute` | An attribute's value does not match its type, or its label would not survive the round trip |
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
  -d '{"visibilityTimeout": 30}'
```

```json
{"error":{"code":"invalid_request_body","message":"Failed to deserialize the JSON body into the target type: visibilityTimeout: unknown field `visibilityTimeout`, expected one of `maxMessages`, `visibilityTimeoutSeconds`, `waitTimeSeconds` at line 1 column 20"}}
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
