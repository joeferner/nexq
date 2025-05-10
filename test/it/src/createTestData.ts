import { NexqApi } from "../generated/client/Api.js";

async function run(): Promise<void> {
  console.log("starting...");
  const client = new NexqApi({
    baseURL: "http://localhost:7887",
  });

  for (let i = 0; i < 30; i++) {
    const queueName = `queue${i}`;
    await client.api.createQueue({
      name: queueName,
    });
    for (let m = 0; m < Math.floor(Math.random() * 50); m++) {
      const attributes: Record<string, string> = {};
      for (let a = 0; a < Math.floor(Math.random() * 5) + 2; a++) {
        attributes[`attr${a}`] = `attribute value ${a}`;
      }
      await client.api.sendMessage(queueName, {
        body: JSON.stringify({
          message:
            "this is a long test string to check display of large strings in the TUI interface when inspecting a message, we want to make sure it wraps correctly. (multi-wide character ⛩️⛩️⛩️⛩️⛩️⛩️⛩️⛩️)",
        }),
        attributes,
      });
    }
  }

  for (let i = 0; i < 30; i++) {
    const topicName = `topic${i}`;
    await client.api.createTopic({
      name: topicName,
    });
  }
}

run().catch(console.error);
