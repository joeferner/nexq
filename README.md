[![NexQ](https://github.com/joeferner/nexq/actions/workflows/node.js.yml/badge.svg)](https://github.com/joeferner/nexq/actions/workflows/node.js.yml)

# About

NexQ is a queue server supporting multiple protocols and multiple storage engines.

# Features

:scroll: - Future
:ballot_box_with_check: - Partially Complete
:white_check_mark: - Completed

## General

- :white_check_mark: Topics and subscriptions
- :white_check_mark: Expiring queues
- :white_check_mark: Delaying messages from receive
- :white_check_mark: Expiring/retention of messages based on time in queue
- :white_check_mark: Dead letter queues and topics
- :white_check_mark: Max message receive count
- :white_check_mark: Configurable NAK behavior (retry, move to end)
- :white_check_mark: Bulk sending/receiving messages
- :white_check_mark: Pause/Resume queues
- :white_check_mark: [Deduplication of messages](docs/features/message-dedup.md)

## Protocols

- :white_check_mark: [REST](packages/proto-rest/README.md)
- :white_check_mark: [Prometheus](packages/proto-prometheus/README.md)
- :white_check_mark: [Keda](packages/proto-keda/README.md)
- :scroll: [AWS (SQS/SNS)](https://aws.amazon.com/pm/sqs/)
- :scroll: AMQP [:link:](https://en.wikipedia.org/wiki/Advanced_Message_Queuing_Protocol)
- :scroll: MQTT [:link:](https://en.wikipedia.org/wiki/MQTT)
- :scroll: OpenWire [:link:](https://en.wikipedia.org/wiki/OpenWire_(binary_protocol))
- :scroll: Stomp [:link:](https://en.wikipedia.org/wiki/Streaming_Text_Oriented_Messaging_Protocol)

## Storage

- :white_check_mark: [Memory](packages/store-memory/README.md)
- :white_check_mark: [SQL](packages/store-sql/README.md)
- :scroll: - Raft [:link:](https://en.wikipedia.org/wiki/Raft_(algorithm))

## Interfaces

- :white_check_mark: [TUI](packages/tui/README.md)

# Links

- [Development](docs/developtment.md)
