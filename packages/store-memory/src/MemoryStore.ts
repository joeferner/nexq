import {
  createLogger,
  CreateQueueOptions,
  CreateTopicOptions,
  CreateUserOptions,
  DEFAULT_MAX_NUMBER_OF_MESSAGES,
  DEFAULT_PASSWORD_HASH_ROUNDS,
  DeleteDeadLetterQueueError,
  DeleteDeadLetterTopicError,
  GetMessage,
  InvalidQueueNameError,
  InvalidTopicNameError,
  Message,
  MoveMessagesResult,
  parseDurationIntoMs,
  PeekMessagesOptions,
  QueueAlreadyExistsError,
  QueueInfo,
  queueInfoEqualCreateQueueOptions,
  QueueNotFoundError,
  ReceivedMessage,
  ReceiveMessageOptions,
  ReceiveMessagesOptions,
  SendMessageOptions,
  SendMessageResult,
  Store,
  Time,
  Timeout,
  TopicAlreadyExistsError,
  TopicInfo,
  topicInfoEqualCreateTopicOptions,
  TopicNotFoundError,
  TopicProtocol,
  toRequiredPeekMessagesOptions,
  UpdateMessageOptions,
  User,
  UserAccessKeyIdAlreadyExistsError,
  UsernameAlreadyExistsError,
} from "@nexq/core";
import * as R from "radash";
import { MemoryQueue } from "./MemoryQueue.js";
import { MemoryTopic } from "./MemoryTopic.js";
import { MemoryUser } from "./MemoryUser.js";
import { SendMessagesOptions } from "@nexq/core/build/dto/SendMessagesOptions.js";
import { SendMessagesResult } from "@nexq/core/build/dto/SendMessagesResult.js";

const logger = createLogger("MemoryStore");

export const DEFAULT_POLL_INTERVAL = "30s";

/**
 * configure expected in nexq.yaml file
 */
export interface MemoryStoreCreateConfig {
  pollInterval?: string;
}

/**
 * configuring expected when creating a MemoryStore
 */
export interface MemoryStoreCreateOptions {
  time: Time;
  passwordHashRounds?: number;
  initialUsers?: CreateUserOptions[];
  config: MemoryStoreCreateConfig;
}

export class MemoryStore implements Store {
  private readonly time: Time;
  private readonly users: MemoryUser[] = [];
  private readonly queues: Record<string, MemoryQueue> = {};
  private readonly topics: Record<string, MemoryTopic> = {};
  private readonly passwordHashRounds: number;
  private readonly pollIntervalMs: number;
  private pollHandle?: Timeout;

  private constructor(options: MemoryStoreCreateOptions) {
    this.time = options.time;
    this.passwordHashRounds = options.passwordHashRounds ?? DEFAULT_PASSWORD_HASH_ROUNDS;

    let pollIntervalMs = parseDurationIntoMs(options.config.pollInterval ?? DEFAULT_POLL_INTERVAL);
    if (pollIntervalMs < 1000) {
      logger.warn(`minimum poll interval is 1s but found ${pollIntervalMs}ms`);
      pollIntervalMs = 1000;
    }
    this.pollIntervalMs = pollIntervalMs;
  }

  public static async create(options: MemoryStoreCreateOptions): Promise<MemoryStore> {
    const store = new MemoryStore(options);
    if (options.initialUsers) {
      for (const initialUser of options.initialUsers) {
        await store.createUser(initialUser);
      }
    }
    return store;
  }

  public async shutdown(): Promise<void> {
    if (this.pollHandle) {
      this.pollHandle.clear();
      this.pollHandle = undefined;
    }
    logger.info("shutdown");
  }

  public async createQueue(queueName: string, options?: CreateQueueOptions): Promise<void> {
    queueName = queueName.trim();
    if (queueName.length === 0) {
      throw new InvalidQueueNameError(queueName);
    }

    const existingQueue = this.queues[queueName];
    if (options?.upsert !== true) {
      if (existingQueue) {
        const match = queueInfoEqualCreateQueueOptions(existingQueue.getInfo(), options ?? {});
        if (match !== true) {
          throw new QueueAlreadyExistsError(queueName, match.reason);
        }
      }
    }

    if (options?.deadLetterQueueName) {
      if (!(options.deadLetterQueueName in this.queues)) {
        throw new QueueNotFoundError(options.deadLetterQueueName);
      }
    }

    if (options?.deadLetterTopicName) {
      if (!(options.deadLetterTopicName in this.topics)) {
        throw new TopicNotFoundError(options.deadLetterTopicName);
      }
    }

    if (existingQueue) {
      existingQueue.update(options ?? {});
    } else {
      const queue = new MemoryQueue({ name: queueName, time: this.time, ...options });
      this.queues[queueName] = queue;
      logger.info(`created new queue "${queueName}"`);
    }
  }

  public async moveMessages(sourceQueueName: string, targetQueueName: string): Promise<MoveMessagesResult> {
    const sourceQueue = this.getQueueRequired(sourceQueueName);
    const targetQueue = this.getQueueRequired(targetQueueName);
    return sourceQueue.moveMessages(targetQueue);
  }

  public async sendMessage(queueName: string, body: string, options?: SendMessageOptions): Promise<SendMessageResult> {
    const queue = this.getQueueRequired(queueName);
    return queue.sendMessage(undefined, body, options);
  }

  public async sendMessages(queueName: string, options: SendMessagesOptions): Promise<SendMessagesResult> {
    const queue = this.getQueueRequired(queueName);
    const ids: string[] = [];
    for (const message of options.messages) {
      const r = queue.sendMessage(undefined, message.body, message);
      ids.push(r.id);
    }
    return { ids };
  }

  public async receiveMessage(
    queueName: string,
    options?: ReceiveMessageOptions
  ): Promise<ReceivedMessage | undefined> {
    const messages = await this.receiveMessages(queueName, { ...options, maxNumberOfMessages: 1 });
    if (messages.length > 1) {
      throw new Error(`expected 0 or 1 but found ${messages.length} when receiving messages`);
    }
    return messages[0];
  }

  public async receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<ReceivedMessage[]> {
    const queue = this.getQueueRequired(queueName);
    const receiveOptions = { maxNumberOfMessages: DEFAULT_MAX_NUMBER_OF_MESSAGES, ...options };
    if (receiveOptions.maxNumberOfMessages === undefined) {
      receiveOptions.maxNumberOfMessages = DEFAULT_MAX_NUMBER_OF_MESSAGES;
    }
    return queue.receiveMessages(receiveOptions);
  }

  public async peekMessages(queueName: string, options?: PeekMessagesOptions): Promise<Message[]> {
    const queue = this.getQueueRequired(queueName);
    return queue.peekMessages(toRequiredPeekMessagesOptions(options));
  }

  public async getMessage(queueName: string, messageId: string): Promise<GetMessage> {
    const queue = this.getQueueRequired(queueName);
    return queue.getMessage(messageId);
  }

  public async pause(queueName: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.pause();
  }

  public async resume(queueName: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.resume();
  }

  public async poll(): Promise<void> {
    try {
      const now = this.time.getCurrentTime();
      this.removeExpiredQueues(now);
      this.pollQueues(now);
    } finally {
      if (!this.pollHandle && this.pollIntervalMs > 0) {
        this.pollHandle = this.time.setTimeout(() => {
          this.pollHandle = undefined;
          void this.poll();
        }, this.pollIntervalMs);
      }
    }
  }

  private removeExpiredQueues(now: Date): void {
    for (const queueName of Object.keys(this.queues)) {
      const queue = this.queues[queueName];
      if (queue.isExpired(now)) {
        delete this.queues[queueName];
      }
    }
  }

  private pollQueues(now: Date): void {
    for (const queue of Object.values(this.queues)) {
      const expiredMessages = queue.poll(now);
      if (queue.deadLetterQueueName || queue.deadLetterTopicName) {
        const deadLetterQueue = queue.deadLetterQueueName
          ? this.getQueueRequired(queue.deadLetterQueueName)
          : undefined;
        const deadLetterTopic = queue.deadLetterTopicName
          ? this.getTopicRequired(queue.deadLetterTopicName)
          : undefined;
        for (const expiredMessage of expiredMessages) {
          if (deadLetterQueue) {
            logger.debug(`moving message ${expiredMessage.id} to dead letter queue "${deadLetterQueue.name}"`);
            deadLetterQueue.sendMessage(expiredMessage.id, expiredMessage.body, {
              priority: expiredMessage.priority,
              attributes: expiredMessage.attributes,
              lastNakReason: expiredMessage.lastNakReason,
            });
          }
          if (deadLetterTopic) {
            logger.debug(`moving message ${expiredMessage.id} to dead letter topic "${deadLetterTopic.name}"`);
            deadLetterTopic.publishMessage(expiredMessage.body, (queueName) => this.getQueueRequired(queueName), {
              priority: expiredMessage.priority,
              attributes: expiredMessage.attributes,
              lastNakReason: expiredMessage.lastNakReason,
            });
          }
        }
      } else {
        if (logger.isDebugEnabled()) {
          for (const expiredMessage of expiredMessages) {
            logger.debug(`deleting expired message ${expiredMessage.id}`);
          }
        }
      }
    }
  }

  public async updateMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void> {
    const expiresAt = new Date(this.time.getCurrentTime().getTime() + timeMs);
    const queue = this.getQueueRequired(queueName);
    queue.updateMessageVisibilityByReceiptHandle(receiptHandle, expiresAt);
  }

  public async deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.deleteMessageByReceiptHandle(receiptHandle);
  }

  public async getQueueInfo(queueName: string): Promise<QueueInfo> {
    const queue = this.getQueueRequired(queueName);
    return queue.getInfo();
  }

  public async getQueueInfos(): Promise<QueueInfo[]> {
    return R.alphabetical(
      Object.values(this.queues).map((q) => q.getInfo()),
      (q) => q.name.toLocaleLowerCase()
    );
  }

  private getQueueRequired(queueName: string): MemoryQueue {
    const queue = this.queues[queueName];
    if (queue) {
      return queue;
    }
    throw new QueueNotFoundError(queueName);
  }

  private getTopicRequired(topicName: string): MemoryTopic {
    const topic = this.topics[topicName];
    if (topic) {
      return topic;
    }
    throw new TopicNotFoundError(topicName);
  }

  public async updateMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.updateMessage(messageId, receiptHandle, updateMessageOptions);
  }

  public async nakMessage(queueName: string, messageId: string, receiptHandle: string, reason?: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.nakMessage(messageId, receiptHandle, reason);
  }

  public async deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.deleteMessage(messageId, receiptHandle);
  }

  public async deleteQueue(queueName: string): Promise<void> {
    logger.info(`deleting queue: ${queueName}`);
    this.getQueueRequired(queueName);

    for (const queue of Object.values(this.queues)) {
      if (queue.deadLetterQueueName === queueName) {
        throw new DeleteDeadLetterQueueError(queue.name, queueName);
      }
    }

    delete this.queues[queueName];
  }

  public async purgeQueue(queueName: string): Promise<void> {
    logger.info(`purging queue: ${queueName}`);
    const queue = this.getQueueRequired(queueName);
    queue.purge();
  }

  public async createUser(options: CreateUserOptions): Promise<void> {
    const existingUser = this.users.find((u) => u.username === options.username);
    if (existingUser) {
      throw new UsernameAlreadyExistsError(options.username);
    }

    if (options.accessKeyId) {
      const existingUserWithAccessKeyId = this.users.find((u) => u.accessKeyId === options.accessKeyId);
      if (existingUserWithAccessKeyId) {
        throw new UserAccessKeyIdAlreadyExistsError(options.accessKeyId);
      }
    }

    this.users.push(await MemoryUser.createUser(options, this.passwordHashRounds));
  }

  public async getUserByUsername(username: string): Promise<User | undefined> {
    return this.users.find((u) => u.username === username);
  }

  public async getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined> {
    return this.users.find((u) => u.accessKeyId === accessKeyId);
  }

  public async getUsers(): Promise<User[]> {
    return this.users;
  }

  public async createTopic(topicName: string, options?: CreateTopicOptions): Promise<void> {
    topicName = topicName.trim();
    if (topicName.length === 0) {
      throw new InvalidTopicNameError(topicName);
    }

    const requiredOptions: CreateTopicOptions = { ...options };
    if (requiredOptions.tags === undefined) {
      requiredOptions.tags = {};
    }

    const existingTopic = this.topics[topicName];
    if (existingTopic) {
      const m = topicInfoEqualCreateTopicOptions(existingTopic.getInfo(), requiredOptions);
      if (m !== true) {
        throw new TopicAlreadyExistsError(topicName, m.reason);
      }
    }
    this.topics[topicName] = new MemoryTopic(topicName, options ?? {});
  }

  public async getTopicInfo(topicName: string): Promise<TopicInfo> {
    const topic = this.getTopicRequired(topicName);
    return topic.getInfo();
  }

  public async getTopicInfos(): Promise<TopicInfo[]> {
    return R.alphabetical(
      Object.values(this.topics).map((q) => q.getInfo()),
      (t) => t.name.toLocaleLowerCase()
    );
  }

  public async subscribe(topicName: string, protocol: TopicProtocol, target: string): Promise<string> {
    switch (protocol) {
      case TopicProtocol.Queue:
        return this.subscribeQueue(topicName, target);
      default:
        throw new Error(`unexpected topic protocol: ${protocol}`);
    }
  }

  public async subscribeQueue(topicName: string, queueName: string): Promise<string> {
    const topic = this.getTopicRequired(topicName);
    const queue = this.getQueueRequired(queueName);
    return topic.subscribeQueue(queue.name);
  }

  public async publishMessage(topicName: string, body: string, options?: SendMessageOptions): Promise<void> {
    const topic = this.getTopicRequired(topicName);
    topic.publishMessage(body, (queueName) => this.getQueueRequired(queueName), options);
  }

  public async deleteTopic(topicName: string): Promise<void> {
    const topic = this.getTopicRequired(topicName);

    for (const queue of Object.values(this.queues)) {
      if (queue.deadLetterTopicName === topicName) {
        throw new DeleteDeadLetterTopicError(queue.name, topicName);
      }
    }

    delete this.topics[topic.name];
  }
}
