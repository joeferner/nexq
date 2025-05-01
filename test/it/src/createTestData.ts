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
        body: `data ${m}`,
        attributes,
      });
    }
  }
}

run().catch(console.error);
