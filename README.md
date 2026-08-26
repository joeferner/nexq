# About

NexQ is a queue server supporting multiple protocols and multiple storage engines.

Today it speaks **Amazon SQS**, well enough that the unmodified `aws` CLI and the AWS
SDKs drive it by pointing at a different endpoint — no client changes, no shims. It
issues its own credentials and has nothing to do with AWS.

# Quick start

Needs a Rust toolchain, and the `aws` CLI to talk to it.

```sh
make server
```

That builds and runs the server, writing a `nexq.toml` from
[`nexq.example.toml`](nexq.example.toml) if you do not have one:

```
listening facade="aws" address=0.0.0.0:8080
```

In another terminal, point the CLI at it. The credentials are the ones in the config
file NexQ just wrote — they are NexQ's own, not AWS's, and the region can be any string:

```sh
export AWS_ACCESS_KEY_ID=AKIANEXQDEV
export AWS_SECRET_ACCESS_KEY=change-me
export AWS_DEFAULT_REGION=us-east-1
export AWS_ENDPOINT_URL=http://localhost:8080
```

Then use it as you would use SQS:

```sh
$ aws sqs create-queue --queue-name jobs
{
    "QueueUrl": "http://localhost:8080/000000000000/jobs"
}

$ aws sqs send-message --queue-url http://localhost:8080/000000000000/jobs \
    --message-body "hello"
{
    "MD5OfMessageBody": "5d41402abc4b2a76b9719d911017c592",
    "MessageId": "69427abc-781a-4773-9c6b-85fd3e4a9bd3"
}

$ aws sqs receive-message --queue-url http://localhost:8080/000000000000/jobs \
    --wait-time-seconds 5
{
    "Messages": [
        {
            "MessageId": "69427abc-781a-4773-9c6b-85fd3e4a9bd3",
            "ReceiptHandle": "1d67c431-52e6-4293-bc43-3999cd62e6be",
            "MD5OfBody": "5d41402abc4b2a76b9719d911017c592",
            "Body": "hello"
        }
    ]
}
```

Delete the message with its receipt handle to finish with it; leave it and it comes back
when its visibility timeout runs out, which is what makes delivery at-least-once.

**[The AWS facade's README](crates/nexq-api-aws/README.md)** is the reference for what is
supported, how queue URLs are put together, `aws configure` as an alternative to the
environment variables above, and what each error means.

# Configuration

`nexq.toml` in the working directory, or whatever `NEXQ_CONFIG` points at. Every key has
a default except the credentials, so the shortest useful config is:

```toml
[[auth.credentials]]
name = "dev"
key_id = "AKIANEXQDEV"
secret = "change-me"
```

Any key can be overridden by an environment variable, prefixed `NEXQ_` with `__` between
nested keys:

```sh
NEXQ_AWS_API__BIND_ADDR=127.0.0.1:9000 nexq-server
```

[`nexq.example.toml`](nexq.example.toml) lists every setting with its default and says
what each one is for.

# Features

:scroll: - Future
:ballot_box_with_check: - Partially Complete
:white_check_mark: - Completed

## Protocols

- :ballot_box_with_check: [REST](crates/nexq-api-rest/README.md) — NexQ's own API, and
  the one the extensions will live behind. The contract, authentication, and documentation
  are in place; the operation surface is one `receive` so far
- :ballot_box_with_check: [AWS (SQS/SNS)](crates/nexq-api-aws/README.md)

## Storage

- :ballot_box_with_check: [Memory](crates/nexq-store-memory/README.md)
- :scroll: [Search](crates/nexq-store-search/README.md)
- :scroll: [SQL](crates/nexq-store-sql/README.md)

# What works today

The SQS facade handles 14 of SQS's 23 operations: the queue lifecycle, the produce and
consume loop with long polling, visibility timeouts and redelivery, message and queue
attributes, batching, and purge. The remaining 9 are access policies, tagging, and the
dead-letter queue API — all recognised, so a client calling one is told the operation is
not built rather than that it does not exist.

NexQ's own REST API runs alongside it on `:8081`, over the same engine — a message sent
through SQS is receivable through REST, and either facade wakes a consumer long-polling on
the other. What is there is the machinery rather than the surface: a bearer-token
listener, HTTPS, one error envelope, long polling, and an OpenAPI document that is
generated from the routes, committed, and browsable at `/api/v1/docs`. Only `receive` is
built, so the SQS facade is still the one that can drive a queue end to end.

Storage is in memory, so nothing survives a restart, and a single node. Durable backends
and clustering are next.

# Development

```sh
make                 # list every target
make pre-commit      # everything CI runs: fmt, clippy, build, test, docs
make test            # the unit and conformance suites
```

Two acceptance suites drive a real server with real AWS clients, which is the only kind
of test that is evidence about compatibility rather than about our own reading of the
protocol. Both are separate from `make pre-commit`, which is meant to stay quick:

```sh
make acceptance-cli    # the aws CLI — botocore
make acceptance-node   # the AWS SDK for JavaScript, which also validates checksums
```

[`todo.md`](todo.md) is the working plan, and records why particular decisions went the
way they did.

# License

MIT — see [`LICENSE`](LICENSE). Use it, ship it, sell it; keep the copyright notice.

The API documentation page bundles [Scalar](https://github.com/scalar/scalar), which is
also MIT and carries its own notice in
[`crates/nexq-api-rest/assets/scalar/`](crates/nexq-api-rest/assets/scalar/).
