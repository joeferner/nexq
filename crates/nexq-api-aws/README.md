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
- :scroll: `GetQueueAttributes` / `SetQueueAttributes`
- :scroll: `PurgeQueue`
- :scroll: `TagQueue` / `UntagQueue` / `ListQueueTags`

## Message operations

- :white_check_mark: `SendMessage` — body, `DelaySeconds`, and `MessageAttributes`.
  `MessageGroupId`, `MessageDeduplicationId`, and `MessageSystemAttributes` are refused
  rather than dropped, so a client is never told a message was stored with data that was
  thrown away
- :white_check_mark: `ReceiveMessage` — `MaxNumberOfMessages`, a per-request
  `VisibilityTimeout`, and `WaitTimeSeconds` up to 20 seconds. Omitting the wait falls
  back to the queue's `ReceiveMessageWaitTimeSeconds`
- :ballot_box_with_check: Message system attributes on receive, through either
  `AttributeNames` (deprecated) or `MessageSystemAttributeNames`, with `All` and the
  Query protocol's `.*` both understood: `SentTimestamp`, `ApproximateReceiveCount`, and
  `ApproximateFirstReceiveTimestamp`. `SenderId` needs a sending principal NexQ does not
  record yet, so naming it is refused rather than answered with a placeholder
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
- :scroll: `SendMessageBatch` / `DeleteMessageBatch`
- :scroll: `ChangeMessageVisibilityBatch`

## Not planned

- FIFO queues. A `.fifo` name is rejected rather than accepted, so no client is left
  believing it has ordering guarantees that do not exist
- Per-message priority through this facade — SQS has no way to express it, so messages
  sent here take the default priority. The REST API is where priority lives

---

# Running the AWS CLI against NexQ

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
| `NotImplemented` | A real SQS operation that is not built yet |

Run the server with `RUST_LOG=nexq=debug` to see which operation each request routed to
and why anything was rejected.
