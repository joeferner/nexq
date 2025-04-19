import { AbortError, Store } from "@nexq/core";
import { MemoryStore } from "@nexq/store-memory";
import { assertQueueEmpty, assertQueueSize, MockTime } from "@nexq/test";
import { beforeEach, describe, expect, test } from "vitest";
import { createMockRequest, expectHttpError } from "../test-utils.js";
import { ApiV1QueueController } from "./ApiV1QueueController.js";

const QUEUE1_NAME = "test-queue1";
const QUEUE2_NAME = "test-queue2";

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
      await controller.createQueue({ name: QUEUE1_NAME });

      const queue = await store.getQueueInfo(QUEUE1_NAME);
      expect(queue.name).toBe(QUEUE1_NAME);
    });

    test("parse error", async () => {
      await expectHttpError(async () => await controller.createQueue({ name: QUEUE1_NAME, delay: "123z" }), 400);
    });

    test("queue already exists", async () => {
      await store.createQueue(QUEUE1_NAME);
      await expectHttpError(
        async () => await controller.createQueue({ name: QUEUE1_NAME, delay: "1ms" }),
        409,
        "queue already exists: delayMs"
      );
    });

    test("dead letter queue not found", async () => {
      await expectHttpError(
        async () => await controller.createQueue({ name: QUEUE1_NAME, deadLetterQueueName: `bad-queue-name` }),
        404
      );
    });

    test("dead letter topic not found", async () => {
      await expectHttpError(
        async () => await controller.createQueue({ name: QUEUE1_NAME, deadLetterTopicName: `bad-topic-name` }),
        404
      );
    });
  });

  describe("getQueues", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      const resp = await controller.getQueues();
      expect(resp.queues.length).toBe(1);
      expect(resp.queues[0].name).toBe(QUEUE1_NAME);
    });
  });

  describe("getQueue", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      const resp = await controller.getQueue(QUEUE1_NAME);
      expect(resp.name).toBe(QUEUE1_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.getQueue("bad-queue-name"), 404);
    });
  });

  describe("deleteQueue", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await controller.deleteQueue(QUEUE1_NAME);
      expect((await store.getQueueInfos()).length).toBe(0);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.deleteQueue("bad-queue-name"), 404);
    });

    test("queue associated with dead letter queue", async () => {
      await store.createQueue(`${QUEUE1_NAME}-dlq`);
      await store.createQueue(QUEUE1_NAME, { deadLetterQueueName: `${QUEUE1_NAME}-dlq` });
      await expectHttpError(async () => await controller.deleteQueue(`${QUEUE1_NAME}-dlq`), 400);
    });
  });

  describe("sendMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await controller.sendMessage(QUEUE1_NAME, { body: "test" });
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
      const m = await store.receiveMessage(QUEUE1_NAME);
      expect(m!.body).toBe("test");
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.sendMessage("bad-queue-name", { body: "" }), 404);
    });
  });

  describe("purgeQueue", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await store.sendMessage(QUEUE1_NAME, "test");
      await controller.purgeQueue(QUEUE1_NAME);

      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.purgeQueue("bad-queue-name"), 404);
    });
  });

  describe("moveMessages", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await store.createQueue(QUEUE2_NAME);

      await store.sendMessage(QUEUE1_NAME, "test1");
      await store.sendMessage(QUEUE1_NAME, "test2");

      const result = await controller.moveMessages(QUEUE1_NAME, QUEUE2_NAME);
      expect(result.movedMessageCount).toBe(2);

      await assertQueueEmpty(store, QUEUE1_NAME);
      await assertQueueSize(store, QUEUE2_NAME, 2, 0, 0);
    });

    test("queue not found", async () => {
      await store.createQueue(QUEUE1_NAME);
      await store.createQueue(QUEUE2_NAME);

      await expectHttpError(async () => await controller.moveMessages(QUEUE1_NAME, "bad-queue-name"), 404);
      await expectHttpError(async () => await controller.moveMessages("bad-queue-name", QUEUE2_NAME), 404);
    });
  });

  describe("deleteMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test");
      await controller.deleteMessage(QUEUE1_NAME, m.id);

      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test");
      const r = await store.receiveMessage(QUEUE1_NAME);
      expect(r).toBeTruthy();
      await controller.deleteMessage(QUEUE1_NAME, m.id, r!.receiptHandle);

      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("queue not found", async () => {
      await expectHttpError(
        async () => await controller.deleteMessage("bad-queue-name", "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "queue not found"
      );
    });

    test("message id not found", async () => {
      await store.createQueue(QUEUE1_NAME);
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE1_NAME, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE1_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("message found but receipt handle does not match (received)", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await store.receiveMessage(QUEUE1_NAME);
      await expectHttpError(
        async () => await controller.deleteMessage(QUEUE1_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });
  });

  describe("updateMessage", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test", {
        priority: 1,
        attributes: { originalAttr: "originalValue" },
      });
      await controller.updateMessage(QUEUE1_NAME, m.id, undefined, {
        priority: 12,
        attributes: { newAttr: "newAttrValue" },
      });

      const recv = await store.receiveMessage(QUEUE1_NAME);
      expect(recv?.priority).toBe(12);
      expect(recv?.attributes["originalAttr"]).toBeUndefined();
      expect(recv?.attributes["newAttr"]).toBe("newAttrValue");
    });

    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE1_NAME, { receiveMessageWaitTimeMs: 10, visibilityTimeoutMs: 5 * 1000 });
      const m = await store.sendMessage(QUEUE1_NAME, "test", { priority: 1 });
      const r = await store.receiveMessage(QUEUE1_NAME);
      await time.advance(4000);
      expect(r).toBeTruthy();

      await controller.updateMessage(QUEUE1_NAME, m.id, r!.receiptHandle, { priority: 12, visibilityTimeout: "5s" });
      await time.advance(4000);
      await store.poll();

      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);
      await time.advance(1001);
      await store.poll();

      const recv = await store.receiveMessage(QUEUE1_NAME);
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
      await store.createQueue(QUEUE1_NAME);
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE1_NAME, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", undefined, {
            priority: 12,
          }),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE1_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", { priority: 12 }),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("message found but receipt handle does not match (received)", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await store.receiveMessage(QUEUE1_NAME);
      await expectHttpError(
        async () =>
          await controller.updateMessage(QUEUE1_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6", { priority: 12 }),
        404,
        "message found but receipt handle does not match"
      );
    });

    test("update message visibility timeout without receipt handle", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await expectHttpError(
        async () => await controller.updateMessage(QUEUE1_NAME, m.id, undefined, { visibilityTimeout: "5s" }),
        400,
        "cannot update message visibility timeout without providing a receipt handle"
      );
    });
  });

  describe("nakMessage", async () => {
    test("good with receipt handle", async () => {
      await store.createQueue(QUEUE1_NAME, { receiveMessageWaitTimeMs: 10, maxReceiveCount: 1 });
      const m = await store.sendMessage(QUEUE1_NAME, "test", { priority: 1 });
      const r = await store.receiveMessage(QUEUE1_NAME);
      await time.advance(4000);
      expect(r).toBeTruthy();

      await controller.nakMessage(QUEUE1_NAME, m.id, r!.receiptHandle);
      await store.poll();

      await assertQueueEmpty(store, QUEUE1_NAME);
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
      await store.createQueue(QUEUE1_NAME);
      await expectHttpError(
        async () =>
          await controller.nakMessage(
            QUEUE1_NAME,
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6",
            "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
          ),
        404,
        "message id not found"
      );
    });

    test("message found but receipt handle does not match (not received)", async () => {
      await store.createQueue(QUEUE1_NAME);
      const m = await store.sendMessage(QUEUE1_NAME, "test message");
      await expectHttpError(
        async () => await controller.nakMessage(QUEUE1_NAME, m.id, "1effd43f-efc0-64a0-abb1-a262ad6a08d6"),
        404,
        "message found but receipt handle does not match"
      );
    });
  });

  describe("receiveMessages", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await store.sendMessage(QUEUE1_NAME, "test1");
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, "test2");

      const resp = await controller.receiveMessages(QUEUE1_NAME, {}, createMockRequest());
      expect(resp.messages.length).toBe(2);
      resp.messages.sort((a, b) => a.sentTime.localeCompare(b.sentTime));

      const message1 = resp.messages[0];
      expect(message1.body).toBe("test1");
      await controller.deleteMessage(QUEUE1_NAME, message1.id, message1.receiptHandle);

      const message2 = resp.messages[1];
      expect(message2.body).toBe("test2");
      await controller.deleteMessage(QUEUE1_NAME, message2.id, message2.receiptHandle);

      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("request closed", async () => {
      await store.createQueue(QUEUE1_NAME);

      const request = createMockRequest();
      let receivedResponse = false;
      let receivedError: unknown = undefined;
      controller
        .receiveMessages(QUEUE1_NAME, { waitTime: "10s" }, request)
        .then((_resp) => {
          receivedResponse = true;
        })
        .catch((err) => {
          receivedError = err;
        });
      await time.advance(1);
      request.close();
      await time.advance(1);
      expect(receivedResponse).toBeFalsy();
      expect(receivedError).toEqual(new AbortError("receive aborted"));
    });
  });

  describe("peekMessages", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);
      await store.sendMessage(QUEUE1_NAME, "test1");
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, "test2");

      const resp = await controller.peekMessages(QUEUE1_NAME);
      expect(resp.messages.length).toBe(2);
      resp.messages.sort((a, b) => a.sentTime.localeCompare(b.sentTime));

      const message1 = resp.messages[0];
      expect(message1.body).toBe("test1");

      const message2 = resp.messages[1];
      expect(message2.body).toBe("test2");
    });

    test("queue not found", async () => {
      await expectHttpError(async () => await controller.peekMessages("bad-queue-name"), 404);
    });
  });

  describe("pause/resume", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE1_NAME);

      await controller.pauseQueue(QUEUE1_NAME);
      const queueAfterPause = await store.getQueueInfo(QUEUE1_NAME);
      expect(queueAfterPause.paused).toBeTruthy();

      await controller.resumeQueue(QUEUE1_NAME);
      const queueAfterResume = await store.getQueueInfo(QUEUE1_NAME);
      expect(queueAfterResume.paused).toBeFalsy();
    });

    test("queue not found (pause)", async () => {
      await expectHttpError(async () => await controller.pauseQueue("bad-queue-name"), 404);
    });

    test("queue not found (resume)", async () => {
      await expectHttpError(async () => await controller.resumeQueue("bad-queue-name"), 404);
    });
  });
});
