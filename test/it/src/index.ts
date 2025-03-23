import { NexqApi } from "../generated/client/Api.js";

const QUEUE1_NAME = "queue1";

async function run(): Promise<void> {
  console.log("starting...");
  const client = new NexqApi({
    baseURL: "http://localhost:7887",
  });
  await testSendPerformance(client);
  await testReceivePerformance(client);
}

async function testSendPerformance(client: NexqApi<unknown>): Promise<void> {
  console.log("test send performance");
  await client.api.createQueue({ name: QUEUE1_NAME });
  try {
    const messageCount = 10000;
    const startTime = new Date();
    const messageArr = [];
    for (let i = 0; i < messageCount; i++) {
      messageArr.push(`test message ${i}`);
    }
    await Promise.all(
      messageArr.map((message) => {
        return client.api.sendMessage(QUEUE1_NAME, {
          body: message,
        });
      })
    );
    const endTime = new Date();
    const deltaT = endTime.getTime() - startTime.getTime();
    console.log(
      `send ${messageCount.toLocaleString()} in ${(deltaT / 1000).toFixed(2)}s, ${((messageCount / deltaT) * 1000).toFixed(2)} messages/second`
    );
  } finally {
    await client.api.deleteQueue(QUEUE1_NAME);
  }
}

async function testReceivePerformance(client: NexqApi<unknown>): Promise<void> {
  console.log("test receive performance");
  await client.api.createQueue({ name: QUEUE1_NAME });
  try {
    const messageCount = 1000;

    // populate queue with messages
    const messageArr = [];
    for (let i = 0; i < messageCount; i++) {
      messageArr.push(`test message ${i}`);
    }
    await Promise.all(
      messageArr.map((message) => {
        return client.api.sendMessage(QUEUE1_NAME, {
          body: message,
        });
      })
    );

    const startTime = new Date();
    const seenMessages: Record<string, boolean> = {};
    await Promise.all(
      messageArr.map(async (_) => {
        const msg = await client.api.receiveMessages(QUEUE1_NAME, { maxNumberOfMessages: 1 });
        const body = msg.data.messages[0].body;
        if (seenMessages[body]) {
          throw new Error(`already saw message: ${body}`);
        }
        seenMessages[body] = true;
      })
    );
    const endTime = new Date();
    const deltaT = endTime.getTime() - startTime.getTime();
    if (messageArr.length !== Object.keys(seenMessages).length) {
      throw new Error(`expected ${messageArr.length} received messages, but found ${Object.keys(seenMessages).length}`);
    }
    for (const message of messageArr) {
      if (!seenMessages[message]) {
        throw new Error(`message already seen: ${message}`);
      }
      delete seenMessages[message];
    }
    if (Object.keys(seenMessages).length > 0) {
      throw new Error(`did not see ${Object.keys(seenMessages).length} messages`);
    }
    console.log(
      `received ${messageCount.toLocaleString()} in ${(deltaT / 1000).toFixed(2)}s, ${((messageCount / deltaT) * 1000).toFixed(2)} messages/second`
    );
  } finally {
    await client.api.deleteQueue(QUEUE1_NAME);
  }
}

run().catch(console.error);
