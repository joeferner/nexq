# AWS (SQS/SNS) protocol

An SQS- and SNS-compatible facade, so unmodified AWS tooling — the `aws` CLI and any
AWS SDK — can talk to NexQ by pointing at a different endpoint. Nothing here decides
queueing behavior; it translates AWS's wire format to and from the core engine.

# Features

:scroll: - Future
:ballot_box_with_check: - Partially Complete
:white_check_mark: - Completed

## Wire protocol

- :white_check_mark: AWS JSON 1.0 — what `aws-cli` v2 and current SDKs send
  (`X-Amz-Target: AmazonSQS.<Operation>`)
- :scroll: AWS Query/XML — what older SDKs send. A request with no `X-Amz-Target` is
  reported as `MissingAction` rather than a generic parse failure, so it is
  recognisable in logs when it does arrive
- :white_check_mark: SQS error shapes — `__type` in the body and `x-amzn-errortype` in
  the headers, so an SDK reports NexQ's errors the way it reports AWS's

## Authentication

- :white_check_mark: SigV4 signature verification against NexQ's own credential
  registry — canonical request rebuilt, HMAC recomputed, compared in constant time.
  Verified against a signature captured from real `botocore`
- :white_check_mark: `InvalidClientTokenId`, `SignatureDoesNotMatch`, and
  `MissingAuthenticationToken`, worded as SQS words them, revealing nothing about
  which half of a credential was wrong
- :white_check_mark: Any region string, since signer and verifier only need to agree
- :white_check_mark: HTTPS, via `[aws_api.tls]`. Handshakes happen off the accept path, so
  a client that connects and then says nothing cannot stop the server accepting others
- :white_check_mark: Mutual TLS, via `client_ca` — a transport-level gate on top of
  SigV4, not a replacement for it. A certificate says a connection is allowed; it never
  says who the caller is
- :white_check_mark: Stale signatures rejected with `RequestTimeTooSkewed`, within a
  configurable window (`aws_api.max_clock_skew_secs`, 15 minutes by default, matching
  AWS). Checked before the signature is recomputed, and in both directions, so a
  timestamp from the future does not stay valid until the clock catches up
- :scroll: Per-principal authorization. Every authenticated principal may do
  everything

## Queue operations

- :ballot_box_with_check: `CreateQueue` — idempotent when attributes match, an error
  when they differ. `VisibilityTimeout`, `DelaySeconds`, and
  `ReceiveMessageWaitTimeSeconds` are supported; other attributes and tags are refused
  rather than ignored
- :white_check_mark: `DeleteQueue`
- :white_check_mark: `GetQueueUrl` — a real lookup, so a missing queue fails here
  rather than on the client's next request
- :white_check_mark: `ListQueues` — `QueueNamePrefix`, plus `MaxResults`/`NextToken`
  paging that botocore's own paginator walks. Paging is by cursor, so queues created or
  deleted between pages cannot make a caller skip or repeat one. A `NextToken` comes
  back whenever more queues remain, even if `MaxResults` was not given — real SQS only
  returns one when it was, but silently truncating at the 1000 cap seemed worse
- :ballot_box_with_check: `GetQueueAttributes` — `VisibilityTimeout`, `DelaySeconds`,
  `ReceiveMessageWaitTimeSeconds`, the three `ApproximateNumberOfMessages*` counts,
  `CreatedTimestamp`, `LastModifiedTimestamp`, `MaximumMessageSize`, and `QueueArn`.
  Attributes that describe features NexQ does not have — `MessageRetentionPeriod`,
  `RedrivePolicy`, the FIFO and encryption sets — are refused with a reason when named,
  and omitted from `All`
- :white_check_mark: `SetQueueAttributes` — a partial update, so naming one attribute
  leaves the rest as they were, and all-or-nothing, so a request that mixes a supported
  attribute with an unsupported one changes neither
- :white_check_mark: `PurgeQueue` — empties a queue, keeps the queue. Takes in-flight
  messages too, so a handle held across a purge stops working. No sixty-second rate
  limit: SQS needs one because its purge is asynchronous, and this one is done when it
  answers
- :scroll: `TagQueue` / `UntagQueue` / `ListQueueTags` — a string map NexQ would store
  and attach no meaning to. Worth building for infrastructure-as-code, which sets tags,
  and for nothing else
- :scroll: `ListDeadLetterSourceQueues`, `StartMessageMoveTask`,
  `CancelMessageMoveTask`, `ListMessageMoveTasks` — the API around dead-letter queues,
  arriving with dead-letter queues rather than before them

## Message operations

- :white_check_mark: `SendMessage` — body, `DelaySeconds`, and `MessageAttributes`,
  including the `NexQ.Priority` attribute described below. `MessageGroupId`,
  `MessageDeduplicationId`, and `MessageSystemAttributes` are refused rather than dropped,
  so a client is never told a message was stored with data that was thrown away
- :white_check_mark: `ReceiveMessage` — `MaxNumberOfMessages`, a per-request
  `VisibilityTimeout`, and `WaitTimeSeconds` up to 20 seconds. Omitting the wait falls
  back to the queue's `ReceiveMessageWaitTimeSeconds`
- :ballot_box_with_check: Message system attributes on receive, through either
  `AttributeNames` (deprecated) or `MessageSystemAttributeNames`, with `All` and the
  Query protocol's `.*` both understood: `SentTimestamp`, `ApproximateReceiveCount`, and
  `ApproximateFirstReceiveTimestamp`. `SenderId` needs a sending principal NexQ does not
  record yet, so naming it is refused rather than answered with a placeholder
- :white_check_mark: **Per-message priority**, NexQ's own extension, as the well-known
  `NexQ.Priority` message attribute — a whole number, higher served first, absent meaning
  the middle of the road. It is **read and kept**, not consumed: an SDK checksums the
  attributes it sent, so an attribute the server removed would make a client reject a
  message that was stored correctly. A value that is not a whole number refuses the send
  rather than quietly defaulting, and the `nexq.` namespace is reserved in any casing, so
  `nexq.priorty` is an error naming the right spelling instead of an attribute nobody
  reads. On receive the *effective* priority — including that of a message sent through
  REST, which carries no such attribute — is answered as a system attribute of the same
  name, and **only when it is named**: `All` returns what real SQS returns, so a client
  that did not ask for the extension never sees it
- :white_check_mark: `DeleteMessage`, with `ReceiptHandleIsInvalid` on a spent handle
- :white_check_mark: `MD5OfMessageBody` on send and `MD5OfBody` on receive — SDKs
  verify these, so they are correctness, not decoration
- :white_check_mark: Visibility timeouts and redelivery — an expired claim makes the
  message claimable again under a new receipt handle, which invalidates the old one
- :white_check_mark: Message attributes — `String`, `Number`, and `Binary`, custom
  labels like `String.uuid` included, carried through untouched and validated by SQS's
  own rules. On receive they are selected with `MessageAttributeNames`: `All`, `.*`,
  exact names, or a `bar.*` prefix
- :white_check_mark: `MD5OfMessageAttributes` on send and receive, matching digests
  published by AWS itself. On receive it covers the attributes actually *returned*, so a
  client asking for a subset can verify what it got
- :white_check_mark: Long polling — the request is held open until a message arrives or
  the wait runs out, woken by the enqueue rather than by a poll. A delay elapsing or a
  visibility timeout lapsing does not wake a waiter yet, since those need a timer rather
  than an event; a consumer sees them on its next receive
- :white_check_mark: `ChangeMessageVisibility` — counted from now, so it extends a claim
  that needs longer and shortens one that does not. A timeout of `0` hands the message
  back at once, and wakes a consumer that is long-polling for it
- :white_check_mark: `SendMessageBatch` / `DeleteMessageBatch` /
  `ChangeMessageVisibilityBatch` — up to ten entries, each succeeding or failing on its
  own, so one bad entry does not sink the rest. `SenderFault` says whether retrying could
  help

## Not planned

- FIFO queues. A `.fifo` name is rejected rather than accepted, so no client is left
  believing it has ordering guarantees that do not exist
- Access policies: `AddPermission`, `RemovePermission`, and the `Policy` attribute.
  Supporting them means evaluating IAM policy documents, which is a second
  authorization model beside NexQ's own credential registry — and two models deciding
  the same question is worse than one. Authorization here will grow out of the
  credential registry instead

---

# Coverage

14 of SQS's 23 operations are implemented, and the other 9 are the three groups above:
access policies, tagging, and the dead-letter queue API.

All 23 are *recognised*, which is the part that matters for a client: an operation that
is real but not built answers `NotImplemented` naming itself, rather than
`UnknownOperationException`, so nobody goes hunting for a typo they did not make.

Every subcommand `aws sqs` offers has been driven against a running NexQ. The 14 work;
the 9 report `NotImplemented`. Worth knowing alongside that: the CLI issues **no implicit
calls** — every request matches a command someone typed, with nothing fetched behind the
scenes — so no operation that works today quietly depends on one of the nine.

---

# Running the AWS CLI against NexQ

The shortest path is in the [root README](../../README.md#quick-start) — four exports and
three commands. This is the fuller version: the alternatives to those exports, what to do
behind a proxy, and what each error means.

## 1. Start a server

From the repository root:

```sh
make server
```

That seeds `nexq.toml` from [`nexq.example.toml`](../../nexq.example.toml) if it is
missing and listens on `0.0.0.0:8080`. The credential in that file is:

```toml
[[auth.credentials]]
name = "dev"
key_id = "AKIANEXQDEV"
secret = "change-me"
```

## 2. Configure a profile

NexQ issues its own credentials and is its own trust root — these have nothing to do
with AWS IAM. The region can be any string, as long as the CLI and NexQ agree on it.

```sh
aws configure --profile nexq
# AWS Access Key ID     [None]: AKIANEXQDEV
# AWS Secret Access Key [None]: change-me
# Default region name   [None]: us-east-1
# Default output format [None]: json
```

Environment variables work just as well:

```sh
export AWS_ACCESS_KEY_ID=AKIANEXQDEV
export AWS_SECRET_ACCESS_KEY=change-me
export AWS_DEFAULT_REGION=us-east-1
```

## 3. Point the CLI at NexQ

Per command:

```sh
aws --profile nexq --endpoint-url http://localhost:8080 sqs list-queues
```

Or once, so `--endpoint-url` can be dropped:

```sh
export AWS_ENDPOINT_URL_SQS=http://localhost:8080
```

Or in `~/.aws/config`:

```ini
[profile nexq]
region = us-east-1
services = nexq

[services nexq]
sqs =
  endpoint_url = http://localhost:8080
```

## 4. Use it

```sh
aws sqs create-queue --queue-name jobs
# { "QueueUrl": "http://localhost:8080/000000000000/jobs" }

aws sqs create-queue --queue-name emails \
  --attributes VisibilityTimeout=120,DelaySeconds=5

aws sqs list-queues
aws sqs list-queues --queue-name-prefix job

aws sqs get-queue-url --queue-name jobs

aws sqs delete-queue --queue-url http://localhost:8080/000000000000/jobs
```

Sending and receiving:

```sh
QUEUE=$(aws sqs create-queue --queue-name jobs --output text)

aws sqs send-message --queue-url "$QUEUE" --message-body "hello world"
# { "MD5OfMessageBody": "5eb63bbb...", "MessageId": "5683a209-..." }

aws sqs receive-message --queue-url "$QUEUE" --max-number-of-messages 10
# { "Messages": [ { "MessageId": ..., "ReceiptHandle": ..., "Body": "hello world" } ] }

aws sqs delete-message --queue-url "$QUEUE" --receipt-handle "<handle>"
```

A received message is invisible to other consumers until its visibility timeout runs
out — 30 seconds by default, or whatever `--visibility-timeout` says. Delete it to
finish; leave it and it comes back, which is what makes delivery at-least-once.

`--visibility-timeout 0` reads messages without keeping them: each is claimable again the
instant it is handed over. One response still holds **distinct** messages, so asking for
ten with a zero timeout returns up to ten different ones rather than the first one ten
times, which is what a queue that ranks its messages would otherwise do.

If the work turns out to take longer than the claim, or cannot be done at all, change
the claim rather than waiting for it to lapse:

```sh
# Needs longer: extend the claim. The receipt handle stays valid.
aws sqs change-message-visibility --queue-url "$QUEUE" \
  --receipt-handle "<handle>" --visibility-timeout 300

# Cannot do it: hand the message straight back for someone else.
aws sqs change-message-visibility --queue-url "$QUEUE" \
  --receipt-handle "<handle>" --visibility-timeout 0
```

The timeout is counted from now rather than from when the message was received, so the
same call shortens a claim as well as extending one. Handing a message back with `0`
makes it claimable immediately and wakes a consumer that is long-polling for it, so work
returned by one consumer reaches the next without waiting.

## Waiting for work

`--wait-time-seconds` holds the request open rather than answering empty straight away,
so a consumer loop is not a busy poll:

```sh
aws sqs receive-message --queue-url "$QUEUE" --wait-time-seconds 20
```

It returns the moment a message is sent, not when the wait runs out, because the send
wakes the waiting consumer directly. If nothing arrives the answer is empty, which is
normal and not an error. Twenty seconds is the maximum, as in SQS.

A queue can make this the default for its consumers, so they need not pass the flag:

```sh
aws sqs create-queue --queue-name jobs \
  --attributes ReceiveMessageWaitTimeSeconds=20
```

An explicit `--wait-time-seconds` overrides it, and `--wait-time-seconds 0` turns
waiting off for one request.

Two things do *not* yet wake a waiting consumer, because they happen when a clock runs
out rather than when a client does something: a `DelaySeconds` delay elapsing, and a
visibility timeout lapsing so a message becomes redeliverable. Both are noticed on the
consumer's next receive.

Shutting the server down releases waiting consumers immediately with an empty response,
rather than holding the shutdown open for up to twenty seconds or dropping their
connections.

To see how many times that has happened, ask for the system attributes:

```sh
aws sqs receive-message --queue-url "$QUEUE" --message-system-attribute-names All
# "Attributes": {
#   "SentTimestamp": "1787753610033",
#   "ApproximateReceiveCount": "2",
#   "ApproximateFirstReceiveTimestamp": "1787753610825"
# }
```

They come back only when asked for, and `--attribute-names` works the same way for
older clients. Timestamps are milliseconds since the epoch, as strings, the way SQS
reports them. `ApproximateReceiveCount` includes the delivery in progress, so a first
receive says `1`, and `ApproximateFirstReceiveTimestamp` stays pinned to the first
delivery rather than moving with the latest.

## Attaching your own metadata

Message attributes are the producer's own key-value data, carried alongside the body:

```sh
cat > attrs.json <<'JSON'
{
  "City":       { "DataType": "String",      "StringValue": "Any City" },
  "Population": { "DataType": "Number",      "StringValue": "1250800" },
  "Label":      { "DataType": "String.uuid", "StringValue": "3f2b1c" },
  "Thumb":      { "DataType": "Binary",      "BinaryValue": "iVBORw0KGgo=" }
}
JSON

aws sqs send-message --queue-url "$QUEUE" --message-body "hello" \
  --message-attributes file://attrs.json
# { "MD5OfMessageBody": "5d41402a...", "MD5OfMessageAttributes": "b972cde9...", ... }

aws sqs receive-message --queue-url "$QUEUE" --message-attribute-names All
aws sqs receive-message --queue-url "$QUEUE" --message-attribute-names City Population
aws sqs receive-message --queue-url "$QUEUE" --message-attribute-names 'bar.*'
```

Like system attributes, they come back only when asked for. `All` and `.*` fetch
everything; a `bar.*` request fetches the family under that prefix. Asking for a name the
message does not carry is not an error — the name is the producer's to choose, so a miss
is just a miss.

`MD5OfMessageAttributes` accompanies them, and on receive it covers **what was
returned**: ask for one of three attributes and you get the digest of that one, so a
client verifying a subset gets an answer that checks out. Binary values travel base64
encoded and are stored as the bytes they decode to, which is what the digest covers.

The rules are SQS's, and NexQ enforces rather than ignores them — up to 10 attributes;
names of at most 256 characters made of letters, digits, `_`, `-`, and `.`, with no
leading, trailing, or doubled period, and not starting with `AWS.` or `Amazon.`; a
`DataType` of `String`, `Number`, or `Binary` with an optional custom label; a `Number`
that really is one; and no empty values. Names, types, and values all count towards the
256 KB message size limit, so metadata cannot be used to smuggle a larger payload.

`list-queues` printing nothing means there are no queues — the same as real SQS, which
omits the field rather than returning an empty list.

## Setting a priority

Priority is NexQ's own idea, and SQS has no field for it. So it travels as one well-known
message attribute — which means an unmodified `aws` CLI or SDK can set it, with no plugin
and no second protocol:

```sh
aws sqs send-message --queue-url "$QUEUE" --message-body "urgent work" \
  --message-attributes 'NexQ.Priority={DataType=Number,StringValue=10}'

aws sqs send-message --queue-url "$QUEUE" --message-body "whenever" \
  --message-attributes 'NexQ.Priority={DataType=Number,StringValue=-5}'

# Higher is served first, so "urgent work" comes back first even though the two were
# sent in the other order.
aws sqs receive-message --queue-url "$QUEUE" --max-number-of-messages 10
```

Any whole number, positive or negative; omitting it means the middle of the road, which
is why every message from a client that knows nothing about NexQ keeps behaving as it
always has. `String` works as well as `Number` — the digits are unambiguous either way —
but a value that is not a whole number **refuses the send** rather than quietly falling
back to the default, because a producer that asked for urgency and silently did not get
it has no way to find out.

The attribute is read and then **kept**: it stays on the message, counts towards the ten
attributes and the size limit like any other, and is returned to consumers that ask for
it. That is not incidental — an SDK checksums the attributes it sent and compares them
against `MD5OfMessageAttributes`, so an attribute the server had quietly removed would
make the client reject a message that was in fact stored correctly.

To read a message's *effective* priority — including one sent through the REST API, which
has a real field for it and so carries no attribute — name it as a system attribute:

```sh
aws sqs receive-message --queue-url "$QUEUE" \
  --message-system-attribute-names NexQ.Priority
# "Attributes": { "NexQ.Priority": "10" }
```

It answers only when named. `--message-system-attribute-names All` deliberately does not
include it: `All` returns what real SQS returns, so a client that did not ask for NexQ's
extension never receives a name AWS has never heard of.

The `nexq.` prefix is reserved in any casing, the way AWS reserves `AWS.` and `Amazon.`.
That is what makes a typo visible: `nexq.priorty` is refused with an error naming the
spelling that works, rather than being stored as an ordinary attribute nobody reads while
the message goes out at the default priority.

## Inspecting and reconfiguring a queue

```sh
aws sqs get-queue-attributes --queue-url "$QUEUE" --attribute-names All
# {
#   "VisibilityTimeout": "120",
#   "DelaySeconds": "5",
#   "ReceiveMessageWaitTimeSeconds": "0",
#   "ApproximateNumberOfMessages": "2",
#   "ApproximateNumberOfMessagesNotVisible": "1",
#   "ApproximateNumberOfMessagesDelayed": "1",
#   "CreatedTimestamp": "1787760116",
#   "LastModifiedTimestamp": "1787760122",
#   "MaximumMessageSize": "262144",
#   "QueueArn": "arn:aws:sqs:us-east-1:000000000000:jobs"
# }

aws sqs get-queue-attributes --queue-url "$QUEUE" \
  --attribute-names ApproximateNumberOfMessages

aws sqs set-queue-attributes --queue-url "$QUEUE" --attributes VisibilityTimeout=600
```

## Batching

Up to ten messages at a time, on all three of send, delete, and change-visibility:

```sh
cat > entries.json <<'JSON'
[
  {"Id": "a", "MessageBody": "one"},
  {"Id": "b", "MessageBody": "two", "DelaySeconds": 30}
]
JSON

aws sqs send-message-batch --queue-url "$QUEUE" --entries file://entries.json
aws sqs delete-message-batch --queue-url "$QUEUE" --entries file://handles.json
aws sqs change-message-visibility-batch --queue-url "$QUEUE" --entries file://retime.json
```

**A batch is not a transaction.** Each entry succeeds or fails on its own, and the
response carries both outcomes — so nine good messages are not lost to one bad one:

```json
{
  "Successful": [
    {"Id": "good", "MessageId": "d26b6968-...", "MD5OfMessageBody": "fff25994..."}
  ],
  "Failed": [
    {"Id": "bad-delay", "SenderFault": true, "Code": "InvalidParameterValue",
     "Message": "DelaySeconds must be between 0 and 900, got 901."}
  ]
}
```

That is still a `200`, so a client has to *look* at `Failed` rather than relying on an
error being raised. `SenderFault` tells it whether retrying could help: `true` means the
request was wrong and will fail again, `false` means this server was and it might not.
Each `Id` is yours, echoed back — it is the only way to tell which entry an outcome
belongs to. `Successful` and `Failed` are omitted when empty.

Five things reject the *whole* batch, and they are all about the list rather than its
contents: no entries (`EmptyBatchRequest`), more than ten
(`TooManyEntriesInBatchRequest`), a repeated `Id` (`BatchEntryIdsNotDistinct`), an `Id`
that is not alphanumeric-plus-`-_` and at most 80 characters (`InvalidBatchEntryId`), and
a `SendMessageBatch` whose messages come to more than 256 KiB in total
(`BatchRequestTooLong` — SQS caps the batch and the individual message at the same size).
A queue that does not exist joins them, since the `QueueUrl` belongs to the request rather
than to any entry.

On `change-message-visibility-batch`, `VisibilityTimeout` is optional per entry — an entry
that omits it gets the queue's configured visibility timeout.

## Purging

To throw away everything in a queue without deleting the queue:

```sh
aws sqs purge-queue --queue-url "$QUEUE"
```

**Irreversible, and it takes in-flight messages with it** — a consumer working on a
message right now will find its receipt handle invalid, because the message is gone. The
queue itself survives with its attributes, which is what separates this from
`delete-queue`.

Unlike SQS there is no sixty-second cooldown: SQS's purge runs asynchronously and it
refuses a second one with `PurgeQueueInProgress` while the first is still going, whereas
this one has finished by the time it answers. Purge twice in a row if you like.

This is the one operation logged at `info` rather than `debug`, with a count of what it
removed, since it is the one that destroys data a client cannot get back:

```
INFO nexq_api_aws::operations: purged queue queue=jobs purged=3
```

The three counts are disjoint and together cover every message the queue holds:
**visible** is claimable now, **not visible** is in flight with a live claim, and
**delayed** is waiting out a `DelaySeconds`. NexQ's numbers are exact rather than
approximate, but they are reported under SQS's names, and a client should not depend on
that.

`CreatedTimestamp` and `LastModifiedTimestamp` are epoch **seconds** — note that message
timestamps such as `SentTimestamp` are milliseconds, which is SQS's inconsistency rather
than NexQ's. A queue that has never been reconfigured reports the same value for both.

`SetQueueAttributes` changes only the attributes it names, leaving the others alone, and
is all-or-nothing: a request mixing a supported attribute with an unsupported one is
refused whole rather than half-applied. Read-only attributes such as `QueueArn` and the
counts are refused rather than quietly ignored.

Only `VisibilityTimeout`, `DelaySeconds`, and `ReceiveMessageWaitTimeSeconds` can be set,
because they are the only ones NexQ has behaviour behind. Asking to read something like
`MessageRetentionPeriod` gets an error saying why — NexQ does not expire messages, so
reporting SQS's four-day default would promise an expiry that never comes.

## Serving HTTPS

```toml
[aws_api]
public_base_url = "https://nexq.internal:8080"   # note the scheme

[aws_api.tls]
certificate = "/etc/nexq/fullchain.pem"
private_key = "/etc/nexq/key.pem"
```

The table's presence is the switch — there is no `enabled` flag, because that would allow
"on, with no certificate", which can only be a mistake. Both paths are read at **startup**,
so a wrong path, an empty file, a file holding two keys, or a key that does not match its
certificate stops the server coming up rather than surfacing later as a client reporting
nothing more useful than "handshake failed".

`certificate` wants the full chain, leaf first. A missing intermediate is the classic
failure that works for some clients and not others.

**Set `public_base_url` to `https://`** as well. Queue URLs are built from it and clients
send every later request to whatever they were handed, so an `http` URL over a TLS
listener points them somewhere nothing is listening. The server warns at startup when the
two disagree rather than refusing, since behind a proxy that terminates or adds TLS they
legitimately can.

A client trusting a privately signed certificate does so its own way — for the CLI:

```sh
aws --ca-bundle /etc/nexq/ca.pem sqs list-queues
```

### Mutual TLS

```toml
[aws_api.tls]
certificate = "/etc/nexq/fullchain.pem"
private_key = "/etc/nexq/key.pem"
client_ca   = "/etc/nexq/client-ca.pem"
```

A connection is then refused during the handshake unless it presents a certificate that
authority signed. **A gate, not an identity**: SigV4 still authenticates every request,
because the SQS protocol requires it and every AWS client signs — so nothing about *who*
a caller is comes from their certificate. It adds a layer rather than replacing one.

Note that the `aws` CLI cannot present a client certificate, so turning this on shuts the
CLI out; it is for SDKs and proxies that can.

## Queue URLs

Queue URLs are `<public_base_url>/<account_id>/<queue-name>`, and clients send the URL
they were given back on every later request. So `aws_api.public_base_url` has to be the
address clients actually reach — behind a container port mapping, an ingress, or a
`kubectl port-forward`, that is not the same as `bind_addr`:

```toml
[aws_api]
bind_addr = "0.0.0.0:8080"
public_base_url = "http://nexq.internal:8080"
```

The host and scheme of an incoming URL are not checked, so a client reaching NexQ by a
different route still works. The account id **is** checked, since a URL carrying a
different one belongs to another deployment.

A queue's ARN, which `GetQueueAttributes` reports, is
`arn:aws:sqs:<region>:<account_id>:<queue-name>`. `aws_api.region` fills that slot and is
used for nothing else — in particular it is not compared against the region a client
signs with, so any region still works:

```toml
[aws_api]
region = "eu-west-2"
```

## When something is refused

| Error | Meaning |
| --- | --- |
| `MissingAuthenticationToken` | The request was not signed at all |
| `InvalidClientTokenId` | No credential with that access key id |
| `SignatureDoesNotMatch` | Wrong secret, or the request was altered after signing |
| `InvalidAddress` | The `QueueUrl` is malformed or names a different account id |
| `QueueDoesNotExist` | No queue by that name |
| `QueueNameExists` | A queue of that name exists with different attributes |
| `ReceiptHandleIsInvalid` | The handle was never issued, is already used, or its claim expired and the message went to another consumer |
| `InvalidAttributeName` | An attribute this facade does not support — a queue attribute such as `FifoQueue`, or a message system attribute it has no value for, such as `SenderId` |
| `RequestTimeTooSkewed` | The client's clock is too far from the server's — the message says by how much |
| `MissingAction` | No `X-Amz-Target`, so probably an older Query-protocol client |
| `NotImplemented` | A real SQS operation that is not built yet — all 23 are recognised, so this is never confused with a typo |
| `EmptyBatchRequest`, `TooManyEntriesInBatchRequest`, `BatchEntryIdsNotDistinct`, `InvalidBatchEntryId`, `BatchRequestTooLong` | The batch itself is malformed, so no entry ran |

Run the server with `RUST_LOG=nexq=debug` to see which operation each request routed to
and why anything was rejected.

---

# The acceptance suite

Everything above is checked by driving the real `aws` CLI against a real server:

```sh
make acceptance-cli      # or: cargo xtask acceptance-cli
```

It builds the server, starts it on a free port, and runs twelve checks — the queue
lifecycle, paging through botocore's own paginator, the produce/consume loop, message and
system attributes, long polling, visibility changes, batches with a deliberate partial
failure, queue attributes and counts, purge, the authentication failures, and
`NotImplemented` on a real operation NexQ does not have.

It needs the `aws` CLI on the path and nothing else — no credentials, since the server is
started from [`nexq.example.toml`](../../nexq.example.toml) and NexQ issues its own. That
also means the example config is checked every run.

CI runs it as its own job. It is not part of `make pre-commit`, which is meant to be
quick: this takes about a minute, most of it the CLI's own startup cost across some fifty
invocations plus the long-poll waits it has to sit through.

The reason for driving the CLI rather than a Rust client is that the CLI is not ours. It
signs its own requests, walks its own paginators, and decides for itself what an error
means — so a check that passes here is evidence about compatibility, where a test
agreeing with our own understanding of SQS is not.

## A second client

```sh
make acceptance-node     # or: cargo xtask acceptance-node
```

The AWS SDK for JavaScript, in [`acceptance/node/`](../../acceptance/node). Passing
against the CLI alone would leave NexQ compatible with *botocore* rather than with SQS's
protocol, and this SDK differs in ways that matter:

- **It validates the MD5s.** `SendMessage`, `SendMessageBatch`, and `ReceiveMessage` all
  recompute the body digest in middleware the client installs by default, and throw
  `Invalid MD5 checksum on messages` rather than returning a result. botocore does not do
  this, so until this suite existed nothing had held NexQ's checksums to a client that
  cares.
- **It deserialises errors into typed classes**, chosen by reading the `__type` field
  NexQ writes. Landing in `QueueDoesNotExist` rather than a generic `SQSServiceException`
  is a check on the error envelope by something that was not written against us.
- **It has its own SigV4 signer**, its own paginator, and its own request timeouts — the
  last of which a twenty-second long poll has to survive.

It needs Node.js and nothing else; the SDK is installed from a committed lockfile on
first run. Seven checks, about nine seconds including the install.
