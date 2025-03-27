import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { MemoryStore } from "@nexq/store-memory";
import { MockTime } from "@nexq/test";
import { createMetrics } from "./serverMetrics.js";

const QUEUE1_NAME = "queue1";

describe("serverMetrics", () => {
  let store!: MemoryStore;

  beforeEach(async () => {
    store = await MemoryStore.create({
      time: new MockTime(),
      config: {
        pollInterval: "10s",
      },
    });
    await store.createQueue(QUEUE1_NAME);
  });

  afterEach(async () => {
    await store.shutdown();
  });

  test("createMetrics", async () => {
    const resultBefore = await createMetrics(store);
    expect(resultBefore).toContain(`nexq_queue_messages{queue="${QUEUE1_NAME}"} 0`);

    await store.sendMessage(QUEUE1_NAME, "test");

    const resultAfterSend = await createMetrics(store);
    expect(resultAfterSend).toContain(`nexq_queue_messages{queue="${QUEUE1_NAME}"} 1`);

    const message = await store.receiveMessage(QUEUE1_NAME);

    const resultAfterReceive = await createMetrics(store);
    expect(resultAfterReceive).toContain(`nexq_queue_messages{queue="${QUEUE1_NAME}"} 1`);

    await store.deleteMessage(QUEUE1_NAME, message!.id, message!.receiptHandle);

    const resultAfterDelete = await createMetrics(store);
    expect(resultAfterDelete).toContain(`nexq_queue_messages{queue="${QUEUE1_NAME}"} 0`);
  });
});
