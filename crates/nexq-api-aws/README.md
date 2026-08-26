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
- :scroll: Rejecting stale signatures. The timestamp is signed but not checked for
  freshness, so **a captured request can be replayed**
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
- :ballot_box_with_check: `ListQueues` — `QueueNamePrefix` is honoured; `MaxResults`
  and `NextToken` are accepted and ignored, so every queue comes back at once
- :scroll: `GetQueueAttributes` / `SetQueueAttributes`
- :scroll: `PurgeQueue`
- :scroll: `TagQueue` / `UntagQueue` / `ListQueueTags`

## Message operations

- :ballot_box_with_check: `SendMessage` — body and `DelaySeconds`. `MessageAttributes`,
  `MessageGroupId`, and `MessageDeduplicationId` are refused rather than dropped, so a
  client is never told a message was stored with data that was thrown away
- :ballot_box_with_check: `ReceiveMessage` — `MaxNumberOfMessages` and a per-request
  `VisibilityTimeout`. `WaitTimeSeconds` is validated and then ignored, so a client
  asking to long-poll gets an immediate empty answer and polls again. Message system
  attributes such as `ApproximateReceiveCount` are not returned yet
- :white_check_mark: `DeleteMessage`, with `ReceiptHandleIsInvalid` on a spent handle
- :white_check_mark: `MD5OfMessageBody` on send and `MD5OfBody` on receive — SDKs
  verify these, so they are correctness, not decoration
- :white_check_mark: Visibility timeouts and redelivery — an expired claim makes the
  message claimable again under a new receipt handle, which invalidates the old one
- :scroll: `SendMessageBatch` / `DeleteMessageBatch`
- :scroll: `ChangeMessageVisibility` / `ChangeMessageVisibilityBatch`
- :scroll: Long polling — the request returns immediately instead of waiting
- :scroll: Message attributes, and the `MD5OfMessageAttributes` that goes with them

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
| `InvalidAttributeName` | An attribute this facade does not support, such as `FifoQueue` |
| `MissingAction` | No `X-Amz-Target`, so probably an older Query-protocol client |
| `NotImplemented` | A real SQS operation that is not built yet |

Run the server with `RUST_LOG=nexq=debug` to see which operation each request routed to
and why anything was rejected.
