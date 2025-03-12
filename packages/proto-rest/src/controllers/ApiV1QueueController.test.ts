import { Store } from "@nexq/core";
import { assertQueueEmpty, assertQueueSize, MockTime } from "@nexq/test";
import { MemoryStore } from "@nexq/store-memory";
import { beforeEach, describe, expect, test } from "vitest";
import { expectHttpError } from "../test-utils.js";
import { ApiV1QueueController } from "./ApiV1QueueController.js";

const QUEUE_NAME = "test-queue";

describe("ApiV1QueueController", async () => {
  let time!: MockTime;
  let store!: Store;
  let controller!: ApiV1QueueController;

  beforeEach(async () => {
    time = new MockTime();
    store = await MemoryStore.create({ time, config: {} });
    controller = new ApiV1QueueController(store);
  });

  describe("createQueue", async () => {
    test("good", async () => {
      await controller.createQueue({ name: QUEUE_NAME });

      const queue = await store.getQueueInfo(QUEUE_NAME);
      expect(queue.name).toBe(QUEUE_NAME);
    });

    test("parse error", async () => {
      await expectHttpError(async () => await controller.createQueue({ name: QUEUE_NAME, delay: "123z" }), 400);
    });

    test("queue already exists", async () => {
      await store.createQueue(QUEUE_NAME);
      await expectHttpError(async () => await controller.createQueue({ name: QUEUE_NAME, delay: "1ms" }), 409);
    });

    test("dead letter queue not found", async () => {
      await expectHttpError(
        async () => await controller.createQueue({ name: QUEUE_NAME, deadLetter: { queueName: `${QUEUE_NAME}-dlq` } }),
        404
      );
    });
  });

  describe("getQueues", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      const resp = await controller.getQueues();
      expect(resp.queues.length).toBe(1);
      expect(resp.queues[0].name).toBe(QUEUE_NAME);
    });
  });

  describe("deleteQueue", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await controller.deleteQueue(QUEUE_NAME);
      expect((await store.getQueueInfos()).length).toBe(0);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.deleteQueue("bad-queue-name"), 404);
    });

    test("queue not found", async () => {
      await store.createQueue(`${QUEUE_NAME}-dlq`);
      await store.createQueue(QUEUE_NAME, { deadLetterQueueName: `${QUEUE_NAME}-dlq` });
      await expectHttpError(async () => await controller.deleteQueue(`${QUEUE_NAME}-dlq`), 400);
    });
  });

  describe("sendMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await controller.sendMessage(QUEUE_NAME, { bodyBase64: btoa("test") });
      await assertQueueSize(store, QUEUE_NAME, 1, 0, 0);
      const m = await store.receiveMessage(QUEUE_NAME);
      expect(m?.bodyAsString()).toBe("test");
    });

    test("base base64 encoding", async () => {
      await store.createQueue(QUEUE_NAME);
      await expectHttpError(async () => await controller.sendMessage(QUEUE_NAME, { bodyBase64: "xyz" }), 400);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.sendMessage("bad-queue-name", { bodyBase64: "" }), 404);
    });
  });

  describe("purgeQueue", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await store.sendMessage(QUEUE_NAME, "test");
      await controller.purgeQueue(QUEUE_NAME);

      await assertQueueEmpty(store, QUEUE_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.purgeQueue("bad-queue-name"), 404);
    });
  });

  describe("deleteMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test");
      await controller.deleteMessage(QUEUE_NAME, m.id);

      await assertQueueEmpty(store, QUEUE_NAME);
    });

    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test");
      const r = await store.receiveMessage(QUEUE_NAME);
      expect(r).toBeTruthy();
      await controller.deleteMessage(QUEUE_NAME, m.id, r!.receiptHandle);

      await assertQueueEmpty(store, QUEUE_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(
        async () => await controller.deleteMessage("bad-queue-name", "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "queue not found"
      );
    });

    test("message id not found", async () => {
      await store.createQueue(QUEUE_NAME);
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE_NAME, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("message found but receipt handle does not match (received)", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await store.receiveMessage(QUEUE_NAME);
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });
  });

  describe("updateMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test", {
        priority: 1,
        attributes: { originalAttr: "originalValue" },
      });
      await controller.updateMessage(QUEUE_NAME, m.id, undefined, {
        priority: 12,
        attributes: { newAttr: "newAttrValue" },
      });

      const recv = await store.receiveMessage(QUEUE_NAME);
      expect(recv?.priority).toBe(12);
      expect(recv?.attributes["originalAttr"]).toBeUndefined();
      expect(recv?.attributes["newAttr"]).toBe("newAttrValue");
    });

    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE_NAME, { receiveMessageWaitTimeMs: 10, visibilityTimeoutMs: 5 * 1000 });
      const m = await store.sendMessage(QUEUE_NAME, "test", { priority: 1 });
      const r = await store.receiveMessage(QUEUE_NAME);
      await time.advance(4000);
      expect(r).toBeTruthy();

      await controller.updateMessage(QUEUE_NAME, m.id, r!.receiptHandle, { priority: 12, visibilityTimeout: "5s" });
      await time.advance(4000);
      await store.poll();

      await assertQueueSize(store, QUEUE_NAME, 0, 0, 1);
      await time.advance(1001);
      await store.poll();

      const recv = await store.receiveMessage(QUEUE_NAME);
      expect(recv?.priority).toBe(12);
    });

    test("queue not found", async () => {
      await expectHttpError(
        async () =>
          await controller.updateMessage("bad-queue-name", "1effd43f-efc0-64a0-abb1-a262ad6a08d6", undefined, {
            priority: 1,
          }),
        404,
        "queue not found"
      );
    });

    test("message id not found", async () => {
      await store.createQueue(QUEUE_NAME);
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE_NAME, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", undefined, {
            priority: 12,
          }),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", { priority: 12 }),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("message found but receipt handle does not match (received)", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await store.receiveMessage(QUEUE_NAME);
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", { priority: 12 }),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("update message visibility timeout without receipt handle", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await expectHttpError(
        async () => await controller.updateMessage(QUEUE_NAME, m.id, undefined, { visibilityTimeout: "5s" }),
        400,
        "cannot update message visibility timeout without providing a receipt handle"
      );
    });
  });

  describe("nakMessage", async () => {
    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE_NAME, { receiveMessageWaitTimeMs: 10, maxReceiveCount: 1 });
      const m = await store.sendMessage(QUEUE_NAME, "test", { priority: 1 });
      const r = await store.receiveMessage(QUEUE_NAME);
      await time.advance(4000);
      expect(r).toBeTruthy();

      await controller.nakMessage(QUEUE_NAME, m.id, r!.receiptHandle);
      await store.poll();

      await assertQueueEmpty(store, QUEUE_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(
        async () =>
          await controller.nakMessage(
            "bad-queue-name",
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6",
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
          ),
        404,
        "queue not found"
      );
    });

    test("message id not found", async () => {
      await store.createQueue(QUEUE_NAME);
      await expectHttpError(
        async () =>
          await controller.nakMessage(
            QUEUE_NAME,
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6",
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
          ),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE_NAME);
      const m = await store.sendMessage(QUEUE_NAME, "test message");
      await expectHttpError(
        async () => await controller.nakMessage(QUEUE_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });
  });

  describe("receiveMessages", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await store.sendMessage(QUEUE_NAME, "test1");
      await time.advance(1);
      await store.sendMessage(QUEUE_NAME, "test2");

      const resp = await controller.receiveMessages(QUEUE_NAME, {});
      expect(resp.messages.length).toBe(2);
      resp.messages.sort((a, b) => a.sentTime.localeCompare(b.sentTime));

      const message1 = resp.messages[0];
      expect(Buffer.from(message1.bodyBase64, "base64").toLocaleString()).toBe("test1");
      await controller.deleteMessage(QUEUE_NAME, message1.id, message1.receiptHandle);

      const message2 = resp.messages[1];
      expect(Buffer.from(message2.bodyBase64, "base64").toLocaleString()).toBe("test2");
      await controller.deleteMessage(QUEUE_NAME, message2.id, message2.receiptHandle);

      await assertQueueEmpty(store, QUEUE_NAME);
    });
  });
});
