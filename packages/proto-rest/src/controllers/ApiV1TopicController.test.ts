import { Store, TopicProtocol } from "@nexq/core";
import { MemoryStore } from "@nexq/store-memory";
import { assertQueueSize, MockTime } from "@nexq/test";
import { beforeEach, describe, expect, test } from "vitest";
import { expectHttpError } from "../test-utils.js";
import { ApiV1TopicController } from "./ApiV1TopicController.js";

const TOPIC_NAME = "test-topic";
const QUEUE_NAME = "test-queue";

describe("ApiV1TopicController", async () => {
  let time!: MockTime;
  let store!: Store;
  let controller!: ApiV1TopicController;

  beforeEach(async () => {
    time = new MockTime();
    store = await MemoryStore.create({ time, config: {} });
    controller = new ApiV1TopicController(store);
  });

  describe("createTopic", async () => {
    test("good", async () => {
      await controller.createTopic({ name: TOPIC_NAME });

      const topic = await store.getTopicInfo(TOPIC_NAME);
      expect(topic.name).toBe(TOPIC_NAME);
    });

    test("topic already exists", async () => {
      await store.createTopic(TOPIC_NAME);
      await expectHttpError(
        async () => await controller.createTopic({ name: TOPIC_NAME, tags: { tag1: "tag1Value" } }),
        409,
        "topic already exists: tags are different"
      );
    });
  });

  describe("getTopics", async () => {
    test("good", async () => {
      await store.createTopic(TOPIC_NAME);
      const resp = await controller.getTopics();
      expect(resp.topics.length).toBe(1);
      expect(resp.topics[0].name).toBe(TOPIC_NAME);
    });
  });

  describe("getTopic", async () => {
    test("good", async () => {
      await store.createTopic(TOPIC_NAME);
      const resp = await controller.getTopic(TOPIC_NAME);
      expect(resp.name).toBe(TOPIC_NAME);
    });

    test("topic not found", async () => {
      await expectHttpError(async () => await controller.getTopic("bad-topic-name"), 404);
    });
  });

  describe("deleteTopic", async () => {
    test("good", async () => {
      await store.createTopic(TOPIC_NAME);
      await controller.deleteTopic(TOPIC_NAME);
      expect((await store.getTopicInfos()).length).toBe(0);
    });

    test("topic not found", async () => {
      await expectHttpError(async () => await controller.deleteTopic("bad-topic-name"), 404);
    });
  });

  describe("publish", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await store.createTopic(TOPIC_NAME);
      await store.subscribe(TOPIC_NAME, TopicProtocol.Queue, QUEUE_NAME);

      await controller.publish(TOPIC_NAME, { body: "test" });

      await assertQueueSize(store, QUEUE_NAME, 1, 0, 0);
      const m = await store.receiveMessage(QUEUE_NAME);
      expect(m?.body).toBe("test");
    });

    test("topic not found", async () => {
      await expectHttpError(async () => await controller.publish("bad-topic-name", { body: "" }), 404);
    });
  });

  describe("subscribe", async () => {
    test("good", async () => {
      await store.createQueue(QUEUE_NAME);
      await store.createTopic(TOPIC_NAME);

      const resp = await controller.subscribeQueue(TOPIC_NAME, { queueName: QUEUE_NAME });

      const topic = await store.getTopicInfo(TOPIC_NAME);
      expect(topic.subscriptions.length).toBe(1);
      expect(topic.subscriptions[0].id).toBe(resp.id);
      expect(topic.subscriptions[0].protocol).toBe(TopicProtocol.Queue);
      expect(topic.subscriptions[0].queueName).toBe(QUEUE_NAME);
    });

    test("topic not found", async () => {
      await expectHttpError(
        async () => await controller.subscribeQueue("bad-topic-name", { queueName: QUEUE_NAME }),
        404
      );
    });

    test("queue not found", async () => {
      await expectHttpError(
        async () => await controller.subscribeQueue(TOPIC_NAME, { queueName: "bad-queue-name" }),
        404
      );
    });
  });
});
