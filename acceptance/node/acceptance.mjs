// The AWS SDK for JavaScript driving NexQ.
//
// A second client, deliberately not the one the other acceptance suite uses. The `aws`
// CLI is botocore, so testing only against it leaves NexQ compatible with one
// implementation of SQS's protocol rather than with the protocol. This SDK is a separate
// implementation of the same thing: its own SigV4 signer, its own error deserialiser,
// its own paginator — and, unlike botocore, its own MD5 validator.
//
// So the checks here are chosen for where the two clients *differ*, not to repeat what
// the CLI already covers.

import {
  BatchEntryIdsNotDistinct,
  ChangeMessageVisibilityCommand,
  CreateQueueCommand,
  DeleteMessageCommand,
  DeleteQueueCommand,
  GetQueueAttributesCommand,
  GetQueueUrlCommand,
  QueueDoesNotExist,
  ReceiptHandleIsInvalid,
  ReceiveMessageCommand,
  SQSClient,
  SendMessageBatchCommand,
  SendMessageCommand,
  paginateListQueues,
} from "@aws-sdk/client-sqs";

const endpoint = process.env.NEXQ_ENDPOINT;
if (!endpoint) {
  console.error("NEXQ_ENDPOINT is not set");
  process.exit(2);
}

const client = new SQSClient({
  endpoint,
  // Any region works: SigV4 only needs signer and verifier to agree on the string.
  region: process.env.AWS_DEFAULT_REGION ?? "us-east-1",
  credentials: {
    accessKeyId: process.env.AWS_ACCESS_KEY_ID,
    secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY,
  },
});

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

// `SendMessageCommand` and `ReceiveMessageCommand` verify the MD5s NexQ reports, in
// middleware the client installs by default. Nothing below asserts on them because it
// does not have to: a mismatch throws `Invalid MD5 checksum on messages` before the
// result is ever returned. That check is the reason this suite exists — botocore does
// not do it, so until now nothing had held NexQ's checksums to a client that cares.
async function roundTripWithChecksumValidation() {
  const url = await queue("node-loop");

  const sent = await client.send(
    new SendMessageCommand({ QueueUrl: url, MessageBody: "hello from node" }),
  );
  // The MD5 of "hello from node". The middleware has already compared this against its
  // own computation by the time the result arrives; asserting the literal value as well
  // means a change to *both* sides would still be caught.
  equal(
    sent.MD5OfMessageBody,
    "a1fd912d6117bb82495853c3a4f615a3",
    "MD5OfMessageBody",
  );
  present(sent.MessageId, "MessageId");

  const received = await client.send(
    new ReceiveMessageCommand({ QueueUrl: url, MaxNumberOfMessages: 1 }),
  );
  const [message] = received.Messages ?? [];
  present(message, "a received message");
  equal(message.Body, "hello from node", "body");
  equal(message.MessageId, sent.MessageId, "message id");

  await client.send(
    new DeleteMessageCommand({
      QueueUrl: url,
      ReceiptHandle: message.ReceiptHandle,
    }),
  );

  const empty = await client.send(new ReceiveMessageCommand({ QueueUrl: url }));
  if (empty.Messages !== undefined) {
    throw new Error("a deleted message came back");
  }
}

// The batch form validates each entry's digest the same way.
async function batchWithChecksumValidation() {
  const url = await queue("node-batch");

  const sent = await client.send(
    new SendMessageBatchCommand({
      QueueUrl: url,
      Entries: [
        { Id: "a", MessageBody: "one" },
        { Id: "b", MessageBody: "two" },
        // Deliberately invalid: the other two must still be sent.
        { Id: "bad", MessageBody: "three", DelaySeconds: 901 },
      ],
    }),
  );

  equal(sent.Successful?.length, 2, "successful entries");
  equal(sent.Failed?.length, 1, "failed entries");
  equal(sent.Failed[0].Id, "bad", "the failed entry's id");
  equal(sent.Failed[0].Code, "InvalidParameterValue", "the failed entry's code");
  equal(sent.Failed[0].SenderFault, true, "SenderFault");
}

// Whether NexQ's error envelope is one a *different* deserialiser understands.
//
// The SDK ships a typed class per SQS error, and picks one by reading the `__type` field
// NexQ writes. Landing in the right class means the envelope is right; a generic
// `SQSServiceException` would mean the shape parsed but the name did not match.
async function typedErrors() {
  const missing = `${endpoint}/000000000000/node-nope`;

  await throws(
    () => client.send(new GetQueueUrlCommand({ QueueName: "node-nope" })),
    QueueDoesNotExist,
    "QueueDoesNotExist",
  );

  const url = await queue("node-errors");
  await client.send(
    new SendMessageCommand({ QueueUrl: url, MessageBody: "hello" }),
  );
  const received = await client.send(new ReceiveMessageCommand({ QueueUrl: url }));
  const handle = received.Messages[0].ReceiptHandle;
  await client.send(new DeleteMessageCommand({ QueueUrl: url, ReceiptHandle: handle }));

  await throws(
    () => client.send(new DeleteMessageCommand({ QueueUrl: url, ReceiptHandle: handle })),
    ReceiptHandleIsInvalid,
    "ReceiptHandleIsInvalid",
  );

  await throws(
    () =>
      client.send(
        new SendMessageBatchCommand({
          QueueUrl: url,
          Entries: [
            { Id: "same", MessageBody: "x" },
            { Id: "same", MessageBody: "y" },
          ],
        }),
      ),
    BatchEntryIdsNotDistinct,
    "BatchEntryIdsNotDistinct",
  );

  // A queue that does not exist, reached by URL rather than by name.
  await throws(
    () => client.send(new SendMessageCommand({ QueueUrl: missing, MessageBody: "x" })),
    QueueDoesNotExist,
    "QueueDoesNotExist via a queue URL",
  );
}

// A held-open request against a client that has its own timeouts.
//
// The CLI tolerating a twenty-second wait says nothing about whether this SDK will: it
// has its own socket and request timeouts, and a default that was too eager would abort
// the request rather than wait for the answer.
async function longPolling() {
  const url = await queue("node-long-poll");

  // Sent while the receive is already blocked, so returning early can only be a wake.
  const sender = new Promise((resolve) => setTimeout(resolve, 3000)).then(() =>
    client.send(
      new SendMessageCommand({ QueueUrl: url, MessageBody: "sent while waiting" }),
    ),
  );

  const started = Date.now();
  const received = await client.send(
    new ReceiveMessageCommand({ QueueUrl: url, WaitTimeSeconds: 20 }),
  );
  const waited = Date.now() - started;
  await sender;

  equal(received.Messages?.[0]?.Body, "sent while waiting", "the message");

  // Loose: the send lands 3 seconds in. Anything well under 20 proves it was woken
  // rather than having waited out its deadline.
  if (waited > 15_000) {
    throw new Error(`the long poll took ${waited}ms, so it was not woken by the send`);
  }
}

// The SDK's own paginator walking NexQ's NextToken.
//
// A second implementation of the same idea as botocore's, and the tokens have to be
// usable by both.
async function pagination() {
  for (let index = 0; index < 7; index += 1) {
    await client.send(new CreateQueueCommand({ QueueName: `node-page-${index}` }));
  }

  const seen = [];
  for await (const page of paginateListQueues(
    { client, pageSize: 2 },
    { QueueNamePrefix: "node-page-" },
  )) {
    seen.push(...(page.QueueUrls ?? []));
  }

  equal(seen.length, 7, "queues found by the paginator");
  equal(new Set(seen).size, 7, "distinct queues, so no page was repeated");
}

async function messageAttributes() {
  const url = await queue("node-attrs");

  const sent = await client.send(
    new SendMessageCommand({
      QueueUrl: url,
      MessageBody: "hello",
      MessageAttributes: {
        City: { DataType: "String", StringValue: "Any City" },
        Population: { DataType: "Number", StringValue: "1250800" },
        // The SDK takes binary as bytes and base64-encodes it on the wire, where the
        // CLI takes it already encoded — a different path to the same request.
        Thumb: { DataType: "Binary", BinaryValue: new Uint8Array([0x89, 0x50, 0x4e, 0x47]) },
      },
    }),
  );
  present(sent.MD5OfMessageAttributes, "MD5OfMessageAttributes");

  const received = await client.send(
    new ReceiveMessageCommand({
      QueueUrl: url,
      MessageAttributeNames: ["All"],
      MessageSystemAttributeNames: ["All"],
    }),
  );
  const [message] = received.Messages ?? [];
  present(message, "a received message");

  equal(Object.keys(message.MessageAttributes ?? {}).length, 3, "attribute count");
  equal(message.MessageAttributes.City.StringValue, "Any City", "a string attribute");
  equal(
    message.MD5OfMessageAttributes,
    sent.MD5OfMessageAttributes,
    "the attribute digest, unchanged by the round trip",
  );

  // Bytes back as bytes, which is what the digest covers.
  const thumb = message.MessageAttributes.Thumb.BinaryValue;
  equal(Buffer.from(thumb).toString("hex"), "89504e47", "the binary attribute");

  // And the system attributes came along, which the SDK exposes under `Attributes`.
  equal(message.Attributes?.ApproximateReceiveCount, "1", "ApproximateReceiveCount");
}

// Priority, set by an SDK that has never heard of NexQ.
//
// Here rather than only in the CLI suite because this is the client that would *notice*
// the tempting shortcut: NexQ reads the `NexQ.Priority` attribute and leaves it on the
// message, and had it consumed the attribute instead, this SDK's MD5 middleware would
// throw `Invalid MD5 checksum on messages` — the digest would cover a different set of
// attributes from the one it sent.
async function priorityWithChecksumValidation() {
  const url = await queue("node-priority");

  // Least urgent first, so first-in-first-out would return them in this order.
  for (const [body, priority] of [
    ["later", "-5"],
    ["urgent", "10"],
  ]) {
    await client.send(
      new SendMessageCommand({
        QueueUrl: url,
        MessageBody: body,
        MessageAttributes: {
          "NexQ.Priority": { DataType: "Number", StringValue: priority },
        },
      }),
    );
  }

  const received = await client.send(
    new ReceiveMessageCommand({
      QueueUrl: url,
      MaxNumberOfMessages: 2,
      MessageAttributeNames: ["All"],
      MessageSystemAttributeNames: ["NexQ.Priority"],
    }),
  );

  equal(received.Messages?.length, 2, "messages returned");
  equal(received.Messages[0].Body, "urgent", "the urgent message is served first");
  equal(
    received.Messages[0].Attributes?.["NexQ.Priority"],
    "10",
    "priority read back as a system attribute",
  );
  equal(
    received.Messages[0].MessageAttributes?.["NexQ.Priority"]?.StringValue,
    "10",
    "the producer's own attribute, kept rather than consumed",
  );
}

async function queueAttributesAndVisibility() {
  const url = await queue("node-queue-attrs");

  const attributes = await client.send(
    new GetQueueAttributesCommand({ QueueUrl: url, AttributeNames: ["All"] }),
  );
  equal(
    attributes.Attributes.QueueArn,
    "arn:aws:sqs:us-east-1:000000000000:node-queue-attrs",
    "QueueArn",
  );
  equal(attributes.Attributes.VisibilityTimeout, "30", "the default visibility timeout");

  // Claimed for twelve hours, handed straight back, claimable again.
  await client.send(new SendMessageCommand({ QueueUrl: url, MessageBody: "work" }));
  const held = await client.send(
    new ReceiveMessageCommand({ QueueUrl: url, VisibilityTimeout: 43_200 }),
  );
  const handle = held.Messages[0].ReceiptHandle;

  const nothing = await client.send(new ReceiveMessageCommand({ QueueUrl: url }));
  if (nothing.Messages !== undefined) {
    throw new Error("a claimed message was handed to a second consumer");
  }

  await client.send(
    new ChangeMessageVisibilityCommand({
      QueueUrl: url,
      ReceiptHandle: handle,
      VisibilityTimeout: 0,
    }),
  );

  const again = await client.send(new ReceiveMessageCommand({ QueueUrl: url }));
  equal(again.Messages?.[0]?.Body, "work", "the handed-back message");

  await client.send(new DeleteQueueCommand({ QueueUrl: url }));
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

const checks = [
  ["round trip, with MD5 validation", roundTripWithChecksumValidation],
  ["batch, with MD5 validation", batchWithChecksumValidation],
  ["typed errors from __type", typedErrors],
  ["long polling", longPolling],
  ["the SDK's paginator", pagination],
  ["message attributes", messageAttributes],
  ["priority, with MD5 validation", priorityWithChecksumValidation],
  ["queue attributes and visibility", queueAttributesAndVisibility],
];

async function queue(name) {
  const created = await client.send(new CreateQueueCommand({ QueueName: name }));

  return created.QueueUrl;
}

function equal(actual, expected, what) {
  if (actual !== expected) {
    throw new Error(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function present(value, what) {
  if (value === undefined || value === null || value === "") {
    throw new Error(`${what} was missing`);
  }
}

async function throws(action, expectedClass, what) {
  try {
    await action();
  } catch (error) {
    if (error instanceof expectedClass) {
      return;
    }
    throw new Error(
      `${what}: expected ${expectedClass.name}, got ${error.constructor.name} (${error.name})`,
    );
  }
  throw new Error(`${what}: expected ${expectedClass.name}, but nothing was thrown`);
}

const failures = [];
for (const [name, check] of checks) {
  try {
    await check();
    console.log(`  ok    ${name}`);
  } catch (error) {
    console.log(`  FAIL  ${name}`);
    console.log(`          ${error.message}`);
    failures.push(name);
  }
}

console.log();
if (failures.length === 0) {
  console.log("all checks passed");
  process.exit(0);
}

console.error(`${failures.length} of the checks failed: ${failures.join(", ")}`);
process.exit(1);
