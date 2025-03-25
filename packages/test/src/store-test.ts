import {
  CreateQueueOptions,
  CreateUserOptions,
  QueueInfo,
  Store,
  Time,
  TopicProtocol,
  verifyPassword,
} from "@nexq/core";
import { afterEach, assert, beforeEach, describe, expect, test } from "vitest";
import { MockTime } from "./MockTime.js";
import * as R from "radash";

const QUEUE1_NAME = "queue1";
const QUEUE2_NAME = "queue2";
const QUEUE3_NAME = "queue3";
const DEAD_LETTER_QUEUE1_NAME = "dataLetterQueue1";
const DEAD_LETTER_QUEUE2_NAME = "dataLetterQueue2";
const TOPIC1_NAME = "topic1";
const TOPIC2_NAME = "topic2";
const MESSAGE1_BODY = "message1";
const MESSAGE2_BODY = "message2";
const MESSAGE3_BODY = "message3";
const MESSAGE4_BODY = "message4";

export interface CreateStoreOptions {
  time: Time;
  initialUsers?: CreateUserOptions[];
}

export async function assertQueueSize(
  store: Store,
  queueName: string,
  numberOfMessages: number,
  numberOfMessagesDelayed: number,
  numberOfMessagesNotVisible: number
): Promise<void> {
  const queueInfo = await store.getQueueInfo(queueName);
  expect(queueInfo.numberOfMessages, "numberOfMessages").toBe(numberOfMessages);
  expect(queueInfo.numberOfMessagesDelayed, "numberOfMessagesDelayed").toBe(numberOfMessagesDelayed);
  expect(queueInfo.numberOfMessagesNotVisible, "numberOfMessagesNotVisible").toBe(numberOfMessagesNotVisible);
}

export async function assertQueueEmpty(store: Store, queueName: string): Promise<void> {
  await assertQueueSize(store, queueName, 0, 0, 0);
}

export async function poll(store: Store): Promise<void> {
  // run simultaneous polls to simulate a multiple server environment
  await Promise.all([store.poll(), store.poll()]);
}

export async function runStoreTest(createStore: (options: CreateStoreOptions) => Promise<Store>): Promise<void> {
  describe("queue", async () => {
    let time!: MockTime;
    let store!: Store;

    beforeEach(async () => {
      time = new MockTime();
      store = await createStore({ time });
    });

    afterEach(async () => {
      await store.shutdown();
    });

    test("peek messages", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // test empty queue
      expect(await store.peekMessages(QUEUE1_NAME)).toEqual([]);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // test 1 item
      const messages = await store.peekMessages(QUEUE1_NAME);
      expect(messages.length).toBe(1);
      expect(messages[0].body).toBe(MESSAGE1_BODY);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
    });

    test("peek messages (includeNotVisible)", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send/receive a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await store.receiveMessage(QUEUE1_NAME);

      // peek without option
      const messagesWithoutOption = await store.peekMessages(QUEUE1_NAME);
      expect(messagesWithoutOption.length).toBe(0);

      // peek with option
      const messages = await store.peekMessages(QUEUE1_NAME, { includeNotVisible: true });
      expect(messages.length).toBe(1);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);
    });

    test("peek messages (include everything)", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send/receive a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await store.receiveMessage(QUEUE1_NAME);
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { delayMs: 100 });

      // peek without option
      const messagesWithoutOption = await store.peekMessages(QUEUE1_NAME);
      expect(messagesWithoutOption.length).toBe(0);

      // peek with option
      const messages = await store.peekMessages(QUEUE1_NAME, { includeNotVisible: true, includeDelayed: true });
      expect(messages.length).toBe(2);
      await assertQueueSize(store, QUEUE1_NAME, 0, 1, 1);
    });

    test("peek messages (max messages)", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send/receive a message
      for (let i = 0; i < 11; i++) {
        await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      }

      // peek
      const messages = await store.peekMessages(QUEUE1_NAME, { maxNumberOfMessages: 10 });
      expect(messages.length).toBe(10);
      await assertQueueSize(store, QUEUE1_NAME, 11, 0, 0);
    });

    test("peek messages (includeDelayed)", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send/receive a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { delayMs: 1000 });

      // peek without option
      const messagesWithoutOption = await store.peekMessages(QUEUE1_NAME);
      expect(messagesWithoutOption.length).toBe(0);

      // peek with option
      const messages = await store.peekMessages(QUEUE1_NAME, { includeDelayed: true });
      expect(messages.length).toBe(1);
      await assertQueueSize(store, QUEUE1_NAME, 0, 1, 0);
    });

    test("get message", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { nakExpireBehavior: "moveToEnd" });

      // send a message
      const originalMessage = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);

      let m = await store.getMessage(QUEUE1_NAME, originalMessage.id);
      expect(m.id).toBe(originalMessage.id);
      expect(m.positionInQueue).toBe(0);
      expect(m.delayUntil).toBeUndefined();
      expect(m.isAvailable).toBeTruthy();
      expect(m.receiveCount).toBe(0);
      expect(m.expiresAt).toBeUndefined();
      expect(m.receiptHandle).toBeUndefined();
      expect(m.firstReceivedAt).toBeUndefined();
      expect(m.lastNakReason).toBeUndefined();

      // send receive more messages
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);
      await time.advance(1);
      const recvMessage = await store.receiveMessage(QUEUE1_NAME);

      m = await store.getMessage(QUEUE1_NAME, originalMessage.id);
      expect(m.id).toBe(originalMessage.id);
      expect(m.positionInQueue).toBe(0);
      expect(m.isAvailable).toBeFalsy();
      expect(m.receiptHandle).toBe(recvMessage?.receiptHandle);

      // nak
      await store.nakMessage(QUEUE1_NAME, m.id, recvMessage!.receiptHandle, "testing");
      await time.advance(1);
      await poll(store);

      m = await store.getMessage(QUEUE1_NAME, originalMessage.id);
      expect(m.id).toBe(originalMessage.id);
      expect(m.positionInQueue).toBe(2);
      expect(m.isAvailable).toBeTruthy();
      expect(m.lastNakReason).toBe("testing");
      expect(m.receiptHandle).toBeUndefined();

      // bad message id
      await expect(async () => await store.getMessage(QUEUE1_NAME, "bad-message-id")).rejects.toThrowError(
        'message id "bad-message-id" is invalid for queue "queue1"'
      );
    });

    test("receive message", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // receive message
      const message = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 5000 });
      assert(message);
      const messageReceiptHandle = message.receiptHandle;
      expect(message.body).toBe(MESSAGE1_BODY);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // try getting more messages
      const moreMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(moreMessage).toBeUndefined();
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // advance before visibility timeout
      await time.advance(4000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // change visibility timeout
      await store.updateMessageVisibilityByReceiptHandle(QUEUE1_NAME, messageReceiptHandle, 5000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // advance time to past visibility timeout
      await time.advance(6000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // since no one picked up the message still allow update visibility timeout
      await store.updateMessageVisibilityByReceiptHandle(QUEUE1_NAME, messageReceiptHandle, 5000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // advance time to past visibility timeout allow others to pick up message
      await time.advance(6000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      const messageAgain = await store.receiveMessage(QUEUE1_NAME);
      expect(messageAgain).toBeTruthy();
      const messageReceiptHandleAgain = messageAgain!.receiptHandle;
      expect(messageAgain!.body).toBe(MESSAGE1_BODY);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // delete message
      await store.deleteMessageByReceiptHandle(QUEUE1_NAME, messageReceiptHandleAgain);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("receive message timeout", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      const recvTime = time.getCurrentTime();
      const recvPromise = store.receiveMessage(QUEUE1_NAME, { waitTimeMs: 1000 });
      await time.advance(1000);
      const message = await recvPromise;
      expect(message).toBeUndefined();
      expect(time.getCurrentTime()).toEqual(new Date(recvTime.getTime() + 1000));
    });

    test("receive message no-timeout", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      const recvPromise = store.receiveMessage(QUEUE1_NAME, { waitTimeMs: 1000 });
      await time.advance(999);
      await poll(store);
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      const message = await recvPromise;
      expect(message!.body).toBe(MESSAGE1_BODY);
    });

    test("queue ordering: fifo", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send messages
      const sentMessage1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      const sentMessage2 = await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);

      // receive message 1
      const message1 = await store.receiveMessage(QUEUE1_NAME);
      assert(message1);
      expect(message1.id).toBe(sentMessage1.id);
      expect(message1.body).toBe(MESSAGE1_BODY);

      // receive message 2
      const message2 = await store.receiveMessage(QUEUE1_NAME);
      assert(message2);
      expect(message2.id).toBe(sentMessage2.id);
      expect(message2.body).toBe(MESSAGE2_BODY);

      // receive message 3 (none)
      const message3 = await store.receiveMessage(QUEUE1_NAME);
      expect(message3).toBeFalsy();
    });

    test("queue ordering: priority", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send messages
      const sentMessage1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      const sentMessage2 = await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY, { priority: 1 });

      // receive message 2
      const message2 = await store.receiveMessage(QUEUE1_NAME);
      assert(message2);
      expect(message2.id).toBe(sentMessage2.id);
      expect(message2.body).toBe(MESSAGE2_BODY);

      // receive message 1
      const message1 = await store.receiveMessage(QUEUE1_NAME);
      assert(message1);
      expect(message1.id).toBe(sentMessage1.id);
      expect(message1.body).toBe(MESSAGE1_BODY);

      // receive message 3 (none)
      const message3 = await store.receiveMessage(QUEUE1_NAME);
      expect(message3).toBeFalsy();
    });

    /**
     * Test to ensure that when a message gets moved to the dead letter it gets added
     * to the end of that queue
     */
    test("queue ordering: dead letter", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, { deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME, maxReceiveCount: 1 });

      // send messages
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      await store.sendMessage(DEAD_LETTER_QUEUE1_NAME, MESSAGE2_BODY);
      await time.advance(1);
      await store.sendMessage(DEAD_LETTER_QUEUE1_NAME, MESSAGE3_BODY);

      // receive message 1 with low visibility timeout to allow timeout
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 1 });

      // let the message timeout
      await time.advance(2);
      await poll(store);

      // at this point we should have message2, message3, message1 in the dead letter queue in that order
      const message2 = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(message2?.body).toBe(MESSAGE2_BODY);
      const message3 = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(message3?.body).toBe(MESSAGE3_BODY);
      const message1 = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(message1?.body).toBe(MESSAGE1_BODY);
    });

    test("queue with expires", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { expiresMs: 60 * 1000 });
      await time.advance(59 * 1000);
      await poll(store);

      // verify receive_messages resets expire
      console.log("a", time.getCurrentTime().toISOString());
      await store.receiveMessage(QUEUE1_NAME);
      console.log("b", time.getCurrentTime().toISOString());
      await time.advance(59 * 1000);
      console.log("c", time.getCurrentTime().toISOString());
      await poll(store);
      console.log("d", time.getCurrentTime().toISOString());
      expect(await store.getQueueInfo(QUEUE1_NAME)).toBeTruthy();

      // verify queue gets deleted, first reset the clock
      await store.receiveMessage(QUEUE1_NAME);
      await time.advance(59 * 1000);
      await poll(store);
      expect(await store.getQueueInfo(QUEUE1_NAME)).toBeTruthy();

      await time.advance(2 * 1000);
      await poll(store);
      await expect(async () => await store.getQueueInfo(QUEUE1_NAME)).rejects.toThrowError(/not found/);
    });

    test("visibility timeout", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // receive the message with visibility timeout
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 5000 });
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      // try receive another message verify we can't
      const message = await store.receiveMessage(QUEUE1_NAME);
      expect(message).toBeUndefined();

      // advance time past visibility timeout
      await time.advance(6000);

      // check that we can now receive the message
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
      const messageAfter = await store.receiveMessage(QUEUE1_NAME);
      assert(messageAfter);
    });

    test("delay", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { delayMs: 10 });
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 1, 0);

      const messageTry1 = await store.receiveMessage(QUEUE1_NAME);
      expect(messageTry1).toBeUndefined();

      await time.advance(11);

      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      const messageTry2 = await store.receiveMessage(QUEUE1_NAME);
      expect(messageTry2).toBeTruthy();
    });

    test("max message size", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { maxMessageSize: 10 });

      // send a message
      await store.sendMessage(QUEUE1_NAME, "1234567890");
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // send a message too big
      await expect(async () => await store.sendMessage(QUEUE1_NAME, "12345678901")).rejects.toThrowError(
        /message of size 11 exceeded the maximum message size of 10/
      );
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
    });

    test("max receive count, no dead letter queue", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { maxReceiveCount: 1 });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // receive once
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 1000 });
      await time.advance(1001);
      await poll(store);

      // message should now be removed
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("no max receive count", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await poll(store);

      for (let i = 0; i < 10; i++) {
        await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
        await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 10 });
        await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);
        await time.advance(11);
        await poll(store);
      }
    });

    test("message retention period, max receive count", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { messageRetentionPeriodMs: 5000, maxReceiveCount: 1 });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // let the message expire
      await time.advance(6000);
      await poll(store);

      // message should now be removed
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("message retention period, no max receive count", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { messageRetentionPeriodMs: 5000 });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // let the message expire
      await time.advance(6000);
      await poll(store);

      // message should now be removed
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("update message", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { visibilityTimeoutMs: 5 * 1000, maxReceiveCount: 1 });

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, {
        priority: 5,
        attributes: { oldAttr: "oldAttrValue" },
      });

      // validate bad message id
      await expect(async () =>
        store.updateMessage(QUEUE1_NAME, "bad-message-id", undefined, { priority: 111 })
      ).rejects.toThrowError(/message id.*is invalid for/);

      // validate bad receipt handle
      await expect(async () =>
        store.updateMessage(QUEUE1_NAME, message1.id, "bad-receipt-handle", { priority: 111 })
      ).rejects.toThrowError(/receipt handle.*is invalid for/);

      // validate can't update visibility timeout without receive handle
      await expect(async () =>
        store.updateMessage(QUEUE1_NAME, message1.id, undefined, { visibilityTimeoutMs: 111 })
      ).rejects.toThrowError(/cannot update message visibility timeout without providing a receipt handle/);

      // update message
      await store.updateMessage(QUEUE1_NAME, message1.id, undefined, {
        priority: 12,
        attributes: { newAttr: "newAttrValue" },
      });

      // check update
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage1?.priority).toBe(12);
      expect(recvMessage1?.attributes["oldAttr"]).toBeUndefined();
      expect(recvMessage1?.attributes["newAttr"]).toBe("newAttrValue");
      await time.advance(4000);
      await poll(store);

      // update message with receipt handle
      await store.updateMessage(QUEUE1_NAME, message1.id, recvMessage1?.receiptHandle, {
        priority: 22,
        visibilityTimeoutMs: 5000,
      });
      await time.advance(4000);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 1);

      await time.advance(1001);
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("nak message", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { visibilityTimeoutMs: 5 * 1000, maxReceiveCount: 1 });

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // validate bad message id
      await expect(async () => store.nakMessage(QUEUE1_NAME, "bad-message-id", "1")).rejects.toThrowError(
        /message id.*is invalid for/
      );

      // validate bad receipt handle
      await expect(async () => store.nakMessage(QUEUE1_NAME, message1.id, "bad-receipt-handle")).rejects.toThrowError(
        /receipt handle.*is invalid for/
      );

      // validate nak message
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);
      await store.nakMessage(QUEUE1_NAME, message1.id, recvMessage1!.receiptHandle);

      // validate message can't be received
      const m = await store.receiveMessage(QUEUE1_NAME, { waitTimeMs: 0 });
      expect(m).toBeUndefined();

      // validate queue is empty
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("nak message dead letter queue", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, { maxReceiveCount: 1, deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME });

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // validate nak message
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);
      await store.nakMessage(QUEUE1_NAME, message1.id, recvMessage1!.receiptHandle, "test message");
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);

      // validate nak reason
      const deadLetterMessage1 = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(deadLetterMessage1!.lastNakReason).toBe("test message");
    });

    test("nak message dead letter topic", async () => {
      // create the queues/topics
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(DEAD_LETTER_QUEUE2_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, DEAD_LETTER_QUEUE1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, DEAD_LETTER_QUEUE2_NAME);
      await store.createQueue(QUEUE1_NAME, { maxReceiveCount: 1, deadLetterTopicName: TOPIC1_NAME });

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // validate nak message
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);
      await store.nakMessage(QUEUE1_NAME, message1.id, recvMessage1!.receiptHandle, "test message");
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);

      // validate nak reason (queue1)
      const deadLetterMessage1 = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(deadLetterMessage1!.lastNakReason).toBe("test message");

      // validate nak reason (queue2)
      const deadLetterMessage2 = await store.receiveMessage(DEAD_LETTER_QUEUE2_NAME);
      expect(deadLetterMessage2!.lastNakReason).toBe("test message");
    });

    test("nak message to same dead letter queue", async () => {
      // create the queues/topics which end up in same queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, {
        maxReceiveCount: 1,
        deadLetterTopicName: TOPIC1_NAME,
        deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME,
      });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      await poll(store);

      // receive message
      const message = await store.receiveMessage(QUEUE1_NAME);

      // nak message
      await store.nakMessage(QUEUE1_NAME, message!.id, message!.receiptHandle);
      await time.advance(1);
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);

      // verify dead letter queue has message
      await assertQueueSize(store, DEAD_LETTER_QUEUE1_NAME, 2, 0, 0);
    });

    test("nak/expire behavior: default", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, {});

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);
      await time.advance(1);

      // nak message
      let recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);
      await store.nakMessage(QUEUE1_NAME, recvMessage!.id, recvMessage!.receiptHandle);
      await poll(store);

      // validate message 1 received again
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);

      // expire messages
      await time.advance(101);
      await poll(store);

      // validate message 1 received again
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);
    });

    test("nak/expire behavior: move to end", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { nakExpireBehavior: "moveToEnd" });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);
      await time.advance(1);

      // nak message
      let recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);
      await store.nakMessage(QUEUE1_NAME, recvMessage!.id, recvMessage!.receiptHandle);
      await poll(store);

      // validate message 1 received again
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.body).toBe(MESSAGE2_BODY);

      // expire messages
      await time.advance(101);
      await poll(store);

      // validate message 3 received
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.body).toBe(MESSAGE3_BODY);
    });

    test("nak/expire behavior: decrease priority", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { nakExpireBehavior: { decreasePriorityBy: 10 } });

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { priority: 20 });
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY, { priority: 10 });
      await time.advance(1);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY, { priority: 1 });
      await time.advance(1);

      // nak message
      let recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);
      expect(recvMessage?.priority).toBe(20);
      await store.nakMessage(QUEUE1_NAME, recvMessage!.id, recvMessage!.receiptHandle);
      await poll(store); // message 1 should be at 10 now

      // validate message 1 received again: at this point it should have the same
      // priority as message 2 but the ordering time should be sooner
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);
      expect(recvMessage?.priority).toBe(10);
      await store.nakMessage(QUEUE1_NAME, recvMessage!.id, recvMessage!.receiptHandle);
      await poll(store); // message 1 should be at 0 now

      // validate message 2 received
      recvMessage = await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 100 });
      expect(recvMessage?.priority).toBe(10);
      expect(recvMessage?.body).toBe(MESSAGE2_BODY);

      // expire messages
      await time.advance(101);
      await poll(store);

      // validate message 3 received, both 1 and 2 should have priority 0 at this point
      recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.priority).toBe(1);
      expect(recvMessage?.body).toBe(MESSAGE3_BODY);

      recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.priority).toBe(0);
      expect(recvMessage?.body).toBe(MESSAGE1_BODY);

      recvMessage = await store.receiveMessage(QUEUE1_NAME);
      expect(recvMessage?.priority).toBe(0);
      expect(recvMessage?.body).toBe(MESSAGE2_BODY);
    });

    test("dead letter queue with messageRetentionPeriodMs", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME, { messageRetentionPeriodMs: 1000 });
      await store.createQueue(QUEUE1_NAME, { maxReceiveCount: 1, deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME });

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // validate nak message
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);
      await store.nakMessage(QUEUE1_NAME, message1.id, recvMessage1!.receiptHandle);
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);

      // validate message on dead letter
      await assertQueueSize(store, DEAD_LETTER_QUEUE1_NAME, 1, 0, 0);

      // validate message is deleted after message retention period
      await time.advance(1001);
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("delete message by id", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // validate bad message id
      await expect(async () => store.deleteMessage(QUEUE1_NAME, "bad-message-id")).rejects.toThrowError(
        'message id "bad-message-id" is invalid for queue "queue1"'
      );

      // update message
      await store.deleteMessage(QUEUE1_NAME, message1.id);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("delete message by id and receipt handle", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      const message1 = await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      const recvMessage1 = await store.receiveMessage(QUEUE1_NAME);

      // validate bad receipt handle
      await expect(async () =>
        store.deleteMessage(QUEUE1_NAME, message1.id, "bad-receipt-handle")
      ).rejects.toThrowError(`receipt handle "bad-receipt-handle" is invalid for queue "${QUEUE1_NAME}"`);

      // update message
      await store.deleteMessage(QUEUE1_NAME, message1.id, recvMessage1?.receiptHandle);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("dead letter queue", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, { deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME, maxReceiveCount: 2 });

      // send a message
      const attributes = { attr1: "attr1Value", attr2: "attr2Value" };
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { attributes, priority: 5 });

      // receive message once
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 1 });
      await time.advance(2);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);

      // receive message again
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 1 });
      await time.advance(2);
      await poll(store);
      await assertQueueEmpty(store, QUEUE1_NAME);

      // message in dead letter
      await assertQueueSize(store, DEAD_LETTER_QUEUE1_NAME, 1, 0, 0);
      const deadLetterMessage = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(deadLetterMessage?.priority).toBe(5);
      expect(deadLetterMessage?.attributes).toEqual(attributes);
    });

    test("dead letter topic", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(DEAD_LETTER_QUEUE2_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, DEAD_LETTER_QUEUE1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, DEAD_LETTER_QUEUE2_NAME);
      await store.createQueue(QUEUE1_NAME, { deadLetterTopicName: TOPIC1_NAME, maxReceiveCount: 1 });

      // send a message
      const attributes = { attr1: "attr1Value", attr2: "attr2Value" };
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY, { attributes, priority: 5 });

      // receive message
      await store.receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 1 });
      await time.advance(2);
      await poll(store);
      await assertQueueSize(store, QUEUE1_NAME, 0, 0, 0);

      // message in dead letter 1
      await assertQueueSize(store, DEAD_LETTER_QUEUE1_NAME, 1, 0, 0);
      let deadLetterMessage = await store.receiveMessage(DEAD_LETTER_QUEUE1_NAME);
      expect(deadLetterMessage?.priority).toBe(5);
      expect(deadLetterMessage?.attributes).toEqual(attributes);

      // message in dead letter 1
      await assertQueueSize(store, DEAD_LETTER_QUEUE2_NAME, 1, 0, 0);
      deadLetterMessage = await store.receiveMessage(DEAD_LETTER_QUEUE2_NAME);
      expect(deadLetterMessage?.priority).toBe(5);
      expect(deadLetterMessage?.attributes).toEqual(attributes);
    });

    test("create queue missing dead letter queue", async () => {
      // create the queue
      await expect(async () => {
        await store.createQueue(QUEUE1_NAME, { deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME, maxReceiveCount: 2 });
      }).rejects.toThrowError(/not found/);
    });

    test("duplicate queues", async () => {
      const createOptions: Required<Omit<CreateQueueOptions, "deadLetterTopicName">> = {
        upsert: false,
        deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME,
        delayMs: 1,
        messageRetentionPeriodMs: 2,
        visibilityTimeoutMs: 3,
        receiveMessageWaitTimeMs: 4,
        expiresMs: 5,
        maxReceiveCount: 6,
        maxMessageSize: 7,
        nakExpireBehavior: "retry",
        tags: {
          tag1: "tag1Value",
          tag2: "tag2Value",
        },
      };

      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, createOptions);
      const info = await store.getQueueInfo(QUEUE1_NAME);
      expect(info).toEqual({
        name: QUEUE1_NAME,
        numberOfMessages: 0,
        numberOfMessagesDelayed: 0,
        numberOfMessagesNotVisible: 0,
        created: time.getCurrentTime(),
        lastModified: time.getCurrentTime(),
        delayMs: 1,
        expiresMs: 5,
        expiresAt: new Date(time.getCurrentTime().getTime() + 5),
        maxMessageSize: 7,
        messageRetentionPeriodMs: 2,
        nakExpireBehavior: "retry",
        receiveMessageWaitTimeMs: 4,
        visibilityTimeoutMs: 3,
        tags: {
          tag1: "tag1Value",
          tag2: "tag2Value",
        },
        deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME,
        maxReceiveCount: 6,
        paused: false,
      } satisfies QueueInfo);

      // create queue again with same parameters
      await store.createQueue(QUEUE1_NAME, createOptions);

      // create queue again with different parameters
      const checkChangeThrows = async (options: CreateQueueOptions): Promise<void> => {
        await expect(async () => {
          await store.createQueue(QUEUE1_NAME, options);
        }).rejects.toThrowError(/already exists/);
      };

      await checkChangeThrows({ ...createOptions, deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME + "2" });
      await checkChangeThrows({ ...createOptions, delayMs: 42 });
      await checkChangeThrows({ ...createOptions, messageRetentionPeriodMs: 42 });
      await checkChangeThrows({ ...createOptions, visibilityTimeoutMs: 42 });
      await checkChangeThrows({ ...createOptions, receiveMessageWaitTimeMs: 42 });
      await checkChangeThrows({ ...createOptions, expiresMs: 42 });
      await checkChangeThrows({ ...createOptions, maxReceiveCount: 42 });
      await checkChangeThrows({ ...createOptions, maxMessageSize: 42 });
      await checkChangeThrows({ ...createOptions, nakExpireBehavior: "moveToEnd" });
      await checkChangeThrows({
        ...createOptions,
        tags: {
          tag1: "tag1Value",
          tag2: "tag2ValueNew",
        },
      });
    });

    test("upsert queues", async () => {
      const timeBeforeUpdate = time.getCurrentTime();
      await store.createQueue(QUEUE1_NAME, { maxReceiveCount: 5 });
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      const queueBeforeUpdate = await store.getQueueInfo(QUEUE1_NAME);
      expect(queueBeforeUpdate.lastModified).toEqual(timeBeforeUpdate);
      await time.advance(100);

      const timeUpdate = time.getCurrentTime();
      await store.createQueue(QUEUE1_NAME, { upsert: true, maxReceiveCount: 6 });
      const queue = await store.getQueueInfo(QUEUE1_NAME);
      expect(queue.maxReceiveCount).toBe(6);
      expect(queue.lastModified).toEqual(timeUpdate);
      await assertQueueSize(store, QUEUE1_NAME, 1, 0, 0);
    });

    test("duplicate queues (without tags)", async () => {
      await store.createQueue(QUEUE1_NAME, {});
      await store.createQueue(QUEUE1_NAME, {});
    });

    test("delete queue", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);

      // delete queue
      await store.deleteQueue(QUEUE1_NAME);

      // verify queue is deleted
      await expect(async () => await store.deleteQueue(QUEUE1_NAME)).rejects.toThrowError(/not found/);
    });

    test("delete dead letter queue", async () => {
      // create the queue
      await store.createQueue(DEAD_LETTER_QUEUE1_NAME);
      await store.createQueue(QUEUE1_NAME, { deadLetterQueueName: DEAD_LETTER_QUEUE1_NAME });

      // verify cannot delete dead letter queue associated with queue
      await expect(async () => await store.deleteQueue(DEAD_LETTER_QUEUE1_NAME)).rejects.toThrowError(
        `cannot delete dead letter queue "${DEAD_LETTER_QUEUE1_NAME}", associated with queue "${QUEUE1_NAME}"`
      );
    });

    test("get queue infos", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);
      await store.createQueue(QUEUE2_NAME);
      await store.sendMessage(QUEUE2_NAME, "test1");
      await store.sendMessage(QUEUE2_NAME, "test2");
      await store.sendMessage(QUEUE2_NAME, "test3");
      await store.sendMessage(QUEUE2_NAME, "test4");
      await store.sendMessage(QUEUE2_NAME, "test5");
      await store.sendMessage(QUEUE2_NAME, "test-delayed", { delayMs: 100 });
      await store.receiveMessage(QUEUE2_NAME); // test1
      await store.receiveMessage(QUEUE2_NAME); // test2

      // get queue infos
      const queueInfos = await store.getQueueInfos();
      expect(queueInfos.length).toBe(2);

      expect(queueInfos[0].name).toBe(QUEUE1_NAME);
      expect(queueInfos[0].numberOfMessages).toBe(0);
      expect(queueInfos[0].numberOfMessagesNotVisible).toBe(0);
      expect(queueInfos[0].numberOfMessagesDelayed).toBe(0);

      expect(queueInfos[1].name).toBe(QUEUE2_NAME);
      expect(queueInfos[1].numberOfMessages).toBe(3); // test3, test4, test5
      expect(queueInfos[1].numberOfMessagesNotVisible).toBe(2); // test1, test2
      expect(queueInfos[1].numberOfMessagesDelayed).toBe(1); // test-delayed
    });

    test("purge queue", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);
      await store.receiveMessage(QUEUE1_NAME);

      // purge queue
      await store.purgeQueue(QUEUE1_NAME);
      await assertQueueEmpty(store, QUEUE1_NAME);
    });

    test("receive multiple", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send a message
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);

      // receive batch 1
      const batch1Messages = await store.receiveMessages(QUEUE1_NAME, { maxNumberOfMessages: 2 });
      expect(batch1Messages.length).toBe(2);
      expect(batch1Messages[0].body).toBe(MESSAGE1_BODY);
      expect(batch1Messages[1].body).toBe(MESSAGE2_BODY);

      // receive batch 2
      const batch2Messages = await store.receiveMessages(QUEUE1_NAME, { maxNumberOfMessages: 2 });
      expect(batch2Messages.length).toBe(1);
      expect(batch2Messages[0].body).toBe(MESSAGE3_BODY);
    });

    test("move messages", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME, { messageRetentionPeriodMs: 10 * 1000 });
      await store.createQueue(QUEUE2_NAME, { messageRetentionPeriodMs: 1000 });

      // send a messages
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE2_BODY, { delayMs: 100 });
      await store.sendMessage(QUEUE1_NAME, MESSAGE3_BODY);
      await store.sendMessage(QUEUE1_NAME, MESSAGE4_BODY);

      // receive a message
      await store.receiveMessage(QUEUE1_NAME);

      // move messages
      await expect(async () => store.moveMessages(QUEUE1_NAME, "bad-queue-name")).rejects.toThrowError(
        /queue "bad-queue-name" not found/
      );
      await expect(async () => store.moveMessages("bad-queue-name", QUEUE2_NAME)).rejects.toThrowError(
        /queue "bad-queue-name" not found/
      );

      const result = await store.moveMessages(QUEUE1_NAME, QUEUE2_NAME);
      expect(result.movedMessageCount).toBe(2);

      // validate
      await assertQueueSize(store, QUEUE1_NAME, 0, 1, 1);
      await assertQueueSize(store, QUEUE2_NAME, 2, 0, 0);

      // validate retention period set
      await time.advance(1001);
      await poll(store);

      await assertQueueEmpty(store, QUEUE2_NAME);
    });

    test("pause/resume", async () => {
      // create the queue
      await store.createQueue(QUEUE1_NAME);

      // send message then pause
      await store.sendMessage(QUEUE1_NAME, MESSAGE1_BODY);
      await time.advance(10);
      await store.pause(QUEUE1_NAME);
      expect((await store.getQueueInfo(QUEUE1_NAME)).paused).toBeTruthy();
      await time.advance(10);

      // receive without resume
      const p1 = store.receiveMessages(QUEUE1_NAME, { waitTimeMs: 1000 }).then((messages) => {
        return { time: time.getCurrentTime(), messages };
      });
      await time.advance(999);
      await time.advance(1);
      const expectedTime1 = time.getCurrentTime();
      await time.advance(1);
      const results1 = await p1;
      expect(results1.time.toISOString()).toBe(expectedTime1.toISOString());
      expect(results1.messages.length).toBe(0);

      // receive with resume
      const p2 = store.receiveMessages(QUEUE1_NAME, { waitTimeMs: 1000, maxNumberOfMessages: 1 }).then((messages) => {
        return { time: time.getCurrentTime(), messages };
      });
      await time.advance(999);
      await store.resume(QUEUE1_NAME);
      expect((await store.getQueueInfo(QUEUE1_NAME)).paused).toBeFalsy();
      await time.advance(0);
      await R.sleep(100); // need to wait for sql to execute
      const expectedTime2 = time.getCurrentTime();
      await time.advance(2);
      const results2 = await p2;
      expect(results2.time.toISOString()).toBe(expectedTime2.toISOString());
      expect(results2.messages.length).toBe(1);
    });
  });

  describe("topics", async () => {
    let time!: MockTime;
    let store!: Store;

    beforeEach(async () => {
      time = new MockTime();
      store = await createStore({ time });
    });

    afterEach(async () => {
      await store.shutdown();
    });

    test("add topic", async () => {
      // create the topic
      await store.createTopic(TOPIC1_NAME);

      // verify topic created
      const topics = await store.getTopicInfos();
      expect(topics.length).toBe(1);
      expect(topics[0].name).toBe(TOPIC1_NAME);

      // verify we can't create a topic with same name and different options
      await expect(async () => store.createTopic(TOPIC1_NAME, { tags: { tag1: "tag1Value" } })).rejects.toThrowError(
        /topic.*already exists/
      );
    });

    test("duplicate queues", async () => {
      await store.createTopic(TOPIC1_NAME, { tags: { a: "b" } });
      await expect(async () => await store.createTopic(TOPIC1_NAME, { tags: { a: "c" } })).rejects.toThrowError(
        /topic "topic1" already exists: tags are different/
      );
    });

    test("duplicate queues (without tags)", async () => {
      await store.createTopic(TOPIC1_NAME, {});
      await store.createTopic(TOPIC1_NAME, {});
    });

    test("subscribe", async () => {
      // create the topic
      await store.createQueue(QUEUE1_NAME);
      await store.createTopic(TOPIC1_NAME);
      const subscriptionId = await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, QUEUE1_NAME);

      // verify topic created
      const topics = await store.getTopicInfos();
      expect(topics.length).toBe(1);
      expect(topics[0].name).toBe(TOPIC1_NAME);
      expect(topics[0].subscriptions.length).toBe(1);
      expect(topics[0].subscriptions[0].id).toBe(subscriptionId);
      expect(topics[0].subscriptions[0].protocol).toBe(TopicProtocol.Queue);
      expect(topics[0].subscriptions[0].queueName).toBe(QUEUE1_NAME);

      // bad topic name
      await expect(
        async () => await store.subscribe("bad-topic-name", TopicProtocol.Queue, QUEUE1_NAME)
      ).rejects.toThrowError(/topic.*not found/);

      // bad queue name
      await expect(
        async () => await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, "bad-queue-name")
      ).rejects.toThrowError(/queue.*not found/);
    });

    test("get topics", async () => {
      // create the topic
      await store.createQueue(QUEUE1_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.createTopic(TOPIC2_NAME);
      const subscriptionId = await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, QUEUE1_NAME);

      // verify topic created
      const topics = await store.getTopicInfos();
      expect(topics.length).toBe(2);
      expect(topics[0].name).toBe(TOPIC1_NAME);
      expect(topics[0].subscriptions.length).toBe(1);
      expect(topics[0].subscriptions[0].id).toBe(subscriptionId);
      expect(topics[0].subscriptions[0].protocol).toBe(TopicProtocol.Queue);
      expect(topics[0].subscriptions[0].queueName).toBe(QUEUE1_NAME);
      expect(topics[1].name).toBe(TOPIC2_NAME);
    });

    test("delete topic", async () => {
      // create the topic
      await store.createQueue(QUEUE1_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, QUEUE1_NAME);

      // delete topic
      await store.deleteTopic(TOPIC1_NAME);
      await expect(async () => await store.deleteTopic("bad-topic-name")).rejects.toThrowError(/topic.*not found/);

      // verify topic deleted
      const topics = await store.getTopicInfos();
      expect(topics.length).toBe(0);
    });

    test("delete dead letter topic", async () => {
      // create the queue
      await store.createTopic(TOPIC1_NAME);
      await store.createQueue(QUEUE1_NAME, { deadLetterTopicName: TOPIC1_NAME });

      // verify cannot delete dead letter queue associated with queue
      await expect(async () => await store.deleteTopic(TOPIC1_NAME)).rejects.toThrowError(
        `cannot delete dead letter topic "${TOPIC1_NAME}", associated with queue "${QUEUE1_NAME}"`
      );
    });

    test("publish", async () => {
      // create the topic
      await store.createQueue(QUEUE1_NAME);
      await store.createQueue(QUEUE2_NAME);
      await store.createQueue(QUEUE3_NAME);
      await store.createTopic(TOPIC1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, QUEUE1_NAME);
      await store.subscribe(TOPIC1_NAME, TopicProtocol.Queue, QUEUE2_NAME);
      await time.advance(1000);

      // public messages
      await store.publishMessage(TOPIC1_NAME, MESSAGE1_BODY);
      await store.publishMessage(TOPIC1_NAME, MESSAGE2_BODY);
      await expect(async () => await store.publishMessage("bad-topic-name", MESSAGE2_BODY)).rejects.toThrowError(
        /topic.*not found/
      );

      // verify messages in queues
      await assertQueueSize(store, QUEUE1_NAME, 2, 0, 0);
      await assertQueueSize(store, QUEUE2_NAME, 2, 0, 0);
      await assertQueueEmpty(store, QUEUE3_NAME);

      // receive messages
      const q1m1 = await store.receiveMessage(QUEUE1_NAME);
      expect(q1m1?.body).toBe(MESSAGE1_BODY);
      const q1m2 = await store.receiveMessage(QUEUE1_NAME);
      expect(q1m2?.body).toBe(MESSAGE2_BODY);

      const q2m1 = await store.receiveMessage(QUEUE2_NAME);
      expect(q2m1?.body).toBe(MESSAGE1_BODY);
      const q2m2 = await store.receiveMessage(QUEUE2_NAME);
      expect(q2m2?.body).toBe(MESSAGE2_BODY);
    });
  });

  describe("user", async () => {
    let time!: MockTime;
    let store!: Store;

    beforeEach(async () => {
      time = new MockTime();
      store = await createStore({ time });
    });

    afterEach(async () => {
      await store?.shutdown();
    });

    test("initial user", async () => {
      await store.shutdown();
      store = await createStore({ time, initialUsers: [{ username: "user1" }] });

      // verify user
      const users = await store.getUsers();
      expect(users.length).toBe(1);
      expect(users[0].username).toBe("user1");
    });

    test("add user", async () => {
      const username = "user1";
      const password = "user1password";
      const accessKeyId = "user1accessKeyId";
      const secretAccessKey = "user1secretAccessKey";

      await store.createUser({ username, password, accessKeyId, secretAccessKey });

      // check the created user
      const userByUsername = await store.getUserByUsername(username);
      assert(userByUsername);
      expect(userByUsername.username).toBe(username);
      assert(userByUsername.passwordHash);
      expect(verifyPassword(password, userByUsername.passwordHash)).toBeTruthy();

      const userByAccessKeyId = await store.getUserByAccessKeyId(accessKeyId);
      assert(userByAccessKeyId);
      expect(userByAccessKeyId.username).toBe(username);

      // verify user not found
      expect(await store.getUserByUsername("wrong-user-name")).toBeUndefined();
      expect(await store.getUserByAccessKeyId("wrong-user-access-key-id")).toBeUndefined();

      // verify multiple users with same username can't exist
      await expect(async () => await store.createUser({ username })).rejects.toThrowError(
        /user with username.*already exists/
      );

      // verify multiple users with same access key id
      await expect(async () => await store.createUser({ username: "new-user", accessKeyId })).rejects.toThrowError(
        /user with access key id.*already exists/
      );
    });

    test("get users", async () => {
      expect((await store.getUsers()).length).toBe(0);

      // create users
      await store.createUser({ username: "user1" });
      await store.createUser({ username: "user2" });

      // verify users
      const users = await store.getUsers();
      expect(users.length).toBe(2);
      expect(users[0].username).toBe("user1");
      expect(users[1].username).toBe("user2");
    });
  });
}
