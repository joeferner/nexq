import {
  createId,
  createLogger,
  CreateQueueOptions,
  CreateTopicOptions,
  CreateUserOptions,
  DEFAULT_MAX_NUMBER_OF_MESSAGES,
  DEFAULT_MAX_RECEIVE_COUNT,
  DEFAULT_PASSWORD_HASH_ROUNDS,
  DeleteDeadLetterQueueError,
  Message,
  QueueAlreadyExistsError,
  QueueInfo,
  QueueNotFoundError,
  ReceiveMessagesOptions,
  SendMessageOptions,
  SendMessageResult,
  Store,
  Time,
  TopicAlreadyExistsError,
  TopicInfo,
  TopicNotFoundError,
  TopicProtocol,
  UpdateMessageOptions,
  User,
  UserAccessKeyIdAlreadyExistsError,
  UsernameAlreadyExistsError,
} from "@nexq/core";
import * as R from "remeda";
import { MemoryQueue } from "./MemoryQueue.js";
import { MemoryTopic } from "./MemoryTopic.js";
import { MemoryUser } from "./MemoryUser.js";
import { Timeout } from "@nexq/core/build/Time.js";

const logger = createLogger("MemoryStore");

export interface MemoryCreateConfig {
  pollInterval?: number;
}

export interface MemoryCreateOptions extends MemoryCreateConfig {
  time: Time;
  passwordHashRounds?: number;
  initialUser?: CreateUserOptions;
}

export class MemoryStore extends Store {
  private readonly users: MemoryUser[] = [];
  private readonly queues: Record<string, MemoryQueue> = {};
  private readonly topics: Record<string, MemoryTopic> = {};
  private readonly passwordHashRounds: number;
  private readonly pollInterval: number;
  private pollHandle?: Timeout;

  private constructor(options: MemoryCreateOptions) {
    super(options.time);
    this.passwordHashRounds = options.passwordHashRounds ?? DEFAULT_PASSWORD_HASH_ROUNDS;
    this.pollInterval = options.pollInterval ?? 1000;
  }

  public static async create(options: MemoryCreateOptions): Promise<MemoryStore> {
    const store = new MemoryStore(options);
    if (options.initialUser) {
      await store.createUser(options.initialUser);
    }
    return store;
  }

  public async start(): Promise<void> {
    logger.info("started");
    await this.poll();
  }

  public async shutdown(): Promise<void> {
    if (this.pollHandle) {
      this.pollHandle.clear();
      this.pollHandle = undefined;
    }
    logger.info("shutdown");
  }

  public async createQueue(queueName: string, options?: CreateQueueOptions): Promise<string> {
    const requiredOptions: CreateQueueOptions = { ...options };
    if (requiredOptions.maxReceiveCount === undefined) {
      requiredOptions.maxReceiveCount = DEFAULT_MAX_RECEIVE_COUNT;
    }

    const existingQueue = this.queues[queueName];
    if (existingQueue) {
      if (!existingQueue.equalCreateQueueOptions(requiredOptions)) {
        throw new QueueAlreadyExistsError(queueName);
      }
    }

    if (options?.deadLetterQueueName) {
      if (!(options.deadLetterQueueName in this.queues)) {
        throw new QueueNotFoundError(options.deadLetterQueueName);
      }
    }

    const queue = new MemoryQueue({ name: queueName, time: this._time, ...requiredOptions });
    this.queues[queueName] = queue;
    logger.info(`created new queue "${queueName}"`);

    return queue.id;
  }

  public async sendMessage(queueName: string, body: string, options?: SendMessageOptions): Promise<SendMessageResult> {
    const queue = this.getQueueRequired(queueName);
    return queue.sendMessage(body, options);
  }

  public async receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<Message[]> {
    const queue = this.getQueueRequired(queueName);
    const receiveOptions = { maxNumberOfMessages: DEFAULT_MAX_NUMBER_OF_MESSAGES, ...options };
    if (receiveOptions.maxNumberOfMessages === undefined) {
      receiveOptions.maxNumberOfMessages = DEFAULT_MAX_NUMBER_OF_MESSAGES;
    }
    return queue.receiveMessages(receiveOptions);
  }

  public async poll(): Promise<void> {
    try {
      const now = this._time.getCurrentTime();
      this.removeExpiredQueues(now);
      this.pollQueues(now);
    } finally {
      if (!this.pollHandle && this.pollInterval > 0) {
        this.pollHandle = this._time.setTimeout(() => {
          this.pollHandle = undefined;
          void this.poll();
        }, this.pollInterval);
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
      if (queue.deadLetterQueueName) {
        const deadLetterQueue = this.getQueueRequired(queue.deadLetterQueueName);
        for (const expiredMessage of expiredMessages) {
          logger.debug(`moving message ${expiredMessage.id} to dead letter queue "${deadLetterQueue.name}"`);
          deadLetterQueue.sendMessage(expiredMessage.body, {
            priority: expiredMessage.priority,
            attributes: expiredMessage.attributes,
          });
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

  public async changeMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void> {
    const takenUntil = new Date(this._time.getCurrentTime().getTime() + timeMs);
    const queue = this.getQueueRequired(queueName);
    queue.updateMessageVisibilityByReceiptHandle(receiptHandle, takenUntil);
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
    return R.sort(
      Object.values(this.queues).map((q) => q.getInfo()),
      (a, b) => {
        const aName = a.name.toLocaleLowerCase();
        const bName = b.name.toLocaleLowerCase();
        return aName.localeCompare(bName);
      }
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

  public async nakMessage(queueName: string, messageId: string, receiptHandle: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.nakMessage(messageId, receiptHandle);
  }

  public async deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.deleteMessage(messageId, receiptHandle);
  }

  public async deleteQueue(queueName: string): Promise<void> {
    this.getQueueRequired(queueName);

    for (const queue of Object.values(this.queues)) {
      if (queue.deadLetterQueueName === queueName) {
        throw new DeleteDeadLetterQueueError(queue.name, queueName);
      }
    }

    delete this.queues[queueName];
  }

  public async purgeQueue(queueName: string): Promise<void> {
    const queue = this.getQueueRequired(queueName);
    queue.purge();
  }

  public async createUser(options: CreateUserOptions): Promise<void> {
    const existingUser = this.users.find((u) => u.username === options.username);
    if (existingUser) {
      throw new UsernameAlreadyExistsError(options.username);
    }

    if (options.accessKeyId) {
      const existingUserWithAccessKey = this.users.find((u) => u.accessKeyId === options.accessKeyId);
      if (existingUserWithAccessKey) {
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
    const requiredOptions: CreateTopicOptions = { ...options };
    if (requiredOptions.tags === undefined) {
      requiredOptions.tags = {};
    }

    const existingTopic = this.topics[topicName];
    if (existingTopic) {
      if (!existingTopic.equalCreateTopicOptions(requiredOptions)) {
        throw new TopicAlreadyExistsError(topicName);
      }
    }
    this.topics[topicName] = new MemoryTopic(topicName, options ?? {});
  }

  public async getTopicInfos(): Promise<TopicInfo[]> {
    return R.sort(
      Object.values(this.topics).map((q) => q.getInfo()),
      (a, b) => {
        const aName = a.name.toLocaleLowerCase();
        const bName = b.name.toLocaleLowerCase();
        return aName.localeCompare(bName);
      }
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

  public async publishMessage(
    topicName: string,
    body: string,
    options?: SendMessageOptions
  ): Promise<SendMessageResult> {
    const topic = this.getTopicRequired(topicName);
    const queues = topic.queueSubscriptions.map((s) => this.getQueueRequired(s.queueName));
    const id = createId();
    for (const queue of queues) {
      queue.sendMessage(body, options);
    }
    return {
      id,
      sequenceNumber: 0,
    };
  }

  public async deleteTopic(topicName: string): Promise<void> {
    const topic = this.getTopicRequired(topicName);
    delete this.topics[topic.name];
  }
}
