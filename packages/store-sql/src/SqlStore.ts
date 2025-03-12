import {
  createId,
  createLogger,
  CreateQueueOptions,
  CreateTopicOptions,
  CreateUserOptions,
  DEFAULT_PASSWORD_HASH_ROUNDS,
  DeleteDeadLetterQueueError,
  hashPassword,
  InvalidUpdateError,
  Message,
  MessageExceededMaxMessageSizeError,
  QueueAlreadyExistsError,
  QueueInfo,
  queueInfoEqualCreateQueueOptions,
  QueueNotFoundError,
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
  Trigger,
  UpdateMessageOptions,
  User,
  UserAccessKeyIdAlreadyExistsError,
  UsernameAlreadyExistsError,
} from "@nexq/core";
import Database from "better-sqlite3";
import * as R from "radash";
import { SqliteDialect } from "./dialect/SqliteDialect.js";
import { NewQueueMessageEvent } from "./events.js";
import { clearRecord } from "./utils.js";

const logger = createLogger("SqlStore");

/**
 * configure expected in nexq.yaml file
 */
export interface _SqlStoreCreateConfig {
  pollInterval?: number;
  connectionString: string;
}

export interface SqlStoreCreateConfigSqlite extends _SqlStoreCreateConfig {
  dialect: "sqlite";
  options?: Database.Options;
}

export type SqlStoreCreateConfig = SqlStoreCreateConfigSqlite;

/**
 * configuring expected when creating a SqlStore
 */
export interface SqlStoreCreateOptions {
  time: Time;
  passwordHashRounds?: number;
  initialUser?: CreateUserOptions;
  config: SqlStoreCreateConfig;
}

export class SqlStore implements Store {
  private readonly time: Time;
  private readonly passwordHashRounds: number;
  private readonly pollInterval: number;
  private readonly dialect: SqliteDialect;
  private readonly triggers: Trigger<NewQueueMessageEvent>[] = [];
  private readonly cachedQueueInfo: Record<string, QueueInfo> = {};
  private readonly cachedTopicInfo: Record<string, TopicInfo> = {};
  private readonly queueTouched: Record<string, Date> = {};
  private pollHandle?: Timeout;

  private constructor(options: SqlStoreCreateOptions & { _dialect: SqliteDialect }) {
    this.time = options.time;
    this.dialect = options._dialect;
    this.passwordHashRounds = options.passwordHashRounds ?? DEFAULT_PASSWORD_HASH_ROUNDS;
    this.pollInterval = options.config.pollInterval ?? 1000;
  }

  public static async create(options: SqlStoreCreateOptions): Promise<SqlStore> {
    const dialect = await this.createDialect(options);
    const store = new SqlStore({
      ...options,
      _dialect: dialect,
    });
    if (options.initialUser) {
      await store.createUser(options.initialUser);
    }
    return store;
  }

  private static createDialect(options: SqlStoreCreateOptions): Promise<SqliteDialect> {
    switch (options.config.dialect) {
      case "sqlite":
        return SqliteDialect.create(options.config, options.time);
      default:
        throw new Error(`unhandled dialect: ${options.config.dialect}`);
    }
  }

  public async shutdown(): Promise<void> {
    if (this.pollHandle) {
      this.pollHandle.clear();
      this.pollHandle = undefined;
    }
    await this.dialect.shutdown();
  }

  public async createUser(options: CreateUserOptions): Promise<void> {
    const existingUser = await this.dialect.findUserByUsername(options.username);
    if (existingUser) {
      throw new UsernameAlreadyExistsError(options.username);
    }

    if (options.accessKeyId) {
      const existingUserWithAccessKey = await this.dialect.findUserByAccessKeyId(options.accessKeyId);
      if (existingUserWithAccessKey) {
        throw new UserAccessKeyIdAlreadyExistsError(options.accessKeyId);
      }
    }

    await this.dialect.createUser({
      id: createId(),
      username: options.username,
      passwordHash: options.password ? await hashPassword(options.password, this.passwordHashRounds) : null,
      accessKeyId: options.accessKeyId ?? null,
      secretAccessKey: options.secretAccessKey ?? null,
    });
  }

  public getUserByUsername(username: string): Promise<User | undefined> {
    return this.dialect.findUserByUsername(username);
  }

  public getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined> {
    return this.dialect.findUserByAccessKeyId(accessKeyId);
  }

  public getUsers(): Promise<User[]> {
    return this.dialect.findUsers();
  }

  public async createQueue(queueName: string, options?: CreateQueueOptions): Promise<void> {
    const existingQueue = await this.dialect.getQueueInfo(queueName);
    if (existingQueue) {
      if (queueInfoEqualCreateQueueOptions(existingQueue, options ?? {})) {
        return;
      } else {
        throw new QueueAlreadyExistsError(queueName);
      }
    }

    if (options?.deadLetterQueueName) {
      const deadLetterQueue = await this.dialect.getQueueInfo(options.deadLetterQueueName);
      if (!deadLetterQueue) {
        throw new QueueNotFoundError(options.deadLetterQueueName);
      }
    }

    await this.dialect.createQueue(queueName, options);
  }

  public async sendMessage(
    queueName: string,
    body: string | Buffer,
    options?: SendMessageOptions
  ): Promise<SendMessageResult> {
    const id = createId();
    const queueInfo = await this.getCachedQueueInfo(queueName);
    if (R.isString(body)) {
      body = Buffer.from(body);
    }
    if (queueInfo.maxMessageSize && body.length > queueInfo.maxMessageSize) {
      throw new MessageExceededMaxMessageSizeError(body.length, queueInfo.maxMessageSize);
    }
    await this.dialect.sendMessage(queueInfo, id, body, options);
    this.trigger({ type: "new-queue-message", queueName } satisfies NewQueueMessageEvent);
    return { id };
  }

  public async publishMessage(
    topicName: string,
    body: string | Buffer,
    options?: SendMessageOptions
  ): Promise<SendMessageResult> {
    const id = createId();
    const topic = await this.getCachedTopicInfo(topicName);
    const queueInfos = await Promise.all(topic.subscriptions.map((sub) => this.getCachedQueueInfo(sub.queueName)));
    if (R.isString(body)) {
      body = Buffer.from(body);
    }
    for (const queueInfo of queueInfos) {
      if (queueInfo.maxMessageSize && body.length > queueInfo.maxMessageSize) {
        throw new MessageExceededMaxMessageSizeError(body.length, queueInfo.maxMessageSize);
      }
    }

    for (const queueInfo of queueInfos) {
      await this.dialect.sendMessage(queueInfo, id, body, options);
      this.trigger({ type: "new-queue-message", queueName: queueInfo.name } satisfies NewQueueMessageEvent);
    }

    return { id };
  }

  public async receiveMessage(queueName: string, options?: ReceiveMessageOptions): Promise<Message | undefined> {
    const messages = await this.receiveMessages(queueName, { ...options, maxNumberOfMessages: 1 });
    if (messages.length > 1) {
      throw new Error(`expected 0 or 1 but found ${messages.length} when receiving messages`);
    }
    return messages[0];
  }

  public async receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<Message[]> {
    const now = this.time.getCurrentTime();
    const nowMs = now.getTime();
    const queueInfo = await this.getCachedQueueInfo(queueName);
    const visibilityTimeoutMs = options?.visibilityTimeoutMs ?? queueInfo.visibilityTimeoutMs ?? 60;
    const waitTime = options?.waitTimeMs ?? queueInfo.receiveMessageWaitTimeMs ?? 0;
    const endTime = nowMs + waitTime;
    if (queueInfo.expiresMs) {
      if (!this.queueTouched[queueName] || this.queueTouched[queueName] < now) {
        this.queueTouched[queueName] = now;
      }
    }
    while (true) {
      const trigger = new Trigger<NewQueueMessageEvent>(this.time);
      this.triggers.push(trigger);

      const messages = await this.dialect.receiveMessages(queueName, {
        visibilityTimeoutMs,
        count: options?.maxNumberOfMessages ?? 10,
      });
      if (messages.length > 0) {
        return messages;
      }

      const now = this.time.getCurrentTime();
      if (now.getTime() > endTime) {
        return messages;
      }

      const timeLeft = endTime - now.getTime();
      if (timeLeft > 0) {
        await trigger.waitUntil(new Date(endTime));
      } else {
        return messages;
      }
    }
  }

  private trigger(message: NewQueueMessageEvent): void {
    const triggers = [...this.triggers];
    this.triggers.length = 0;
    for (const trigger of triggers) {
      trigger.trigger(message);
    }
  }

  private async getCachedQueueInfo(queueName: string): Promise<QueueInfo> {
    let queueInfo = this.cachedQueueInfo[queueName];
    if (queueInfo) {
      return queueInfo;
    }
    queueInfo = await this.getQueueInfo(queueName);
    if (queueInfo) {
      return queueInfo;
    }
    throw new QueueNotFoundError(queueName);
  }

  private async getCachedTopicInfo(topicName: string): Promise<TopicInfo> {
    let topicInfo = this.cachedTopicInfo[topicName];
    if (topicInfo) {
      return topicInfo;
    }
    topicInfo = await this.getTopicInfo(topicName);
    if (topicInfo) {
      return topicInfo;
    }
    throw new TopicNotFoundError(topicName);
  }

  public async poll(): Promise<void> {
    const reloadQueueInfosCache = async (): Promise<QueueInfo[]> => {
      const queueInfos = await this.dialect.getQueueInfos();
      const queueExpiresToUpdate: { queueName: string; newExpires: Date }[] = [];
      clearRecord(this.cachedQueueInfo);
      for (const queueInfo of queueInfos) {
        this.cachedQueueInfo[queueInfo.name] = queueInfo;
        if (queueInfo.expiresMs && this.queueTouched[queueInfo.name]) {
          const lastTouched = this.queueTouched[queueInfo.name];
          const newExpires = new Date(lastTouched.getTime() + queueInfo.expiresMs);
          queueInfo.expiresAt = newExpires;
          queueExpiresToUpdate.push({ queueName: queueInfo.name, newExpires });
        }
      }
      clearRecord(this.queueTouched);
      await Promise.all(
        queueExpiresToUpdate.map((update) => {
          logger.debug(`updating queue expires at "${update.queueName}" = ${update.newExpires.toISOString()}`);
          return this.dialect.updateQueueExpiresAt(update.queueName, update.newExpires);
        })
      );
      return queueInfos;
    };

    const reloadTopicInfosCache = async (): Promise<TopicInfo[]> => {
      const topicInfos = await this.dialect.getTopicInfos();
      clearRecord(this.cachedTopicInfo);
      for (const topicInfo of topicInfos) {
        this.cachedTopicInfo[topicInfo.name] = topicInfo;
      }
      return topicInfos;
    };

    try {
      const now = this.time.getCurrentTime();
      let queueInfos = await reloadQueueInfosCache();
      await reloadTopicInfosCache();

      for (const queueInfo of queueInfos) {
        if (queueInfo.expiresAt && queueInfo.expiresAt < now) {
          logger.info(`deleting expired queue "${queueInfo.name}"`);
          await this.deleteQueue(queueInfo.name);
          queueInfos = queueInfos.filter((qi) => qi !== queueInfo);
        }
      }

      for (const queueInfo of queueInfos) {
        await this.pollQueue(queueInfo);
      }
    } finally {
      if (!this.pollHandle && this.pollInterval > 0) {
        this.pollHandle = this.time.setTimeout(() => {
          this.pollHandle = undefined;
          void this.poll();
        }, this.pollInterval);
      }
    }
  }

  private async pollQueue(queueInfo: QueueInfo): Promise<void> {
    if (queueInfo.deadLetterQueueName) {
      const deadLetterQueueInfo = await this.getCachedQueueInfo(queueInfo.deadLetterQueueName);
      const results = await this.dialect.moveExpiredMessagesToDeadLetter(queueInfo, deadLetterQueueInfo);
      if (results.changes > 0) {
        logger.debug(
          `moved ${results.changes} messages from "${queueInfo.name}" to dead letter "${queueInfo.deadLetterQueueName}"`
        );
      }
    } else {
      const now = this.time.getCurrentTime();
      if (queueInfo.messageRetentionPeriodMs !== undefined) {
        const results = await this.dialect.deleteExpiredMessagesOverRetention(queueInfo.name, now);
        if (results.changes > 0) {
          logger.debug(`deleted ${results.changes} messages from "${queueInfo.name}" that exceeded message retention`);
        }
      }

      if (queueInfo.maxReceiveCount !== undefined) {
        const results = await this.dialect.deleteExpiredMessagesOverReceiveCount(
          queueInfo.name,
          queueInfo.maxReceiveCount,
          now
        );
        if (results.changes > 0) {
          logger.debug(`deleted ${results.changes} messages from "${queueInfo.name}" that exceeded receive count`);
        }
      }
    }
  }

  public async updateMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void> {
    const queueInfo = await this.getCachedQueueInfo(queueName);
    await this.dialect.updateMessageVisibilityByReceiptHandle(queueInfo.name, receiptHandle, timeMs);
  }

  public async deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void> {
    const queueInfo = await this.getCachedQueueInfo(queueName);
    await this.dialect.deleteMessageByReceiptHandle(queueInfo.name, receiptHandle);
  }

  public getQueueInfo(queueName: string): Promise<QueueInfo> {
    return this.getQueueRequired(queueName);
  }

  public getQueueInfos(): Promise<QueueInfo[]> {
    return this.dialect.getQueueInfos();
  }

  public async updateMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): Promise<void> {
    if (receiptHandle === undefined && updateMessageOptions.visibilityTimeoutMs !== undefined) {
      throw new InvalidUpdateError("cannot update message visibility timeout without providing a receipt handle");
    }
    const queue = await this.getCachedQueueInfo(queueName);
    await this.dialect.updateMessage(queue.name, messageId, receiptHandle, updateMessageOptions);
  }

  public async nakMessage(queueName: string, messageId: string, receiptHandle: string, reason?: string): Promise<void> {
    const now = this.time.getCurrentTime();
    const queue = await this.getCachedQueueInfo(queueName);
    const expiresAt = new Date(now.getTime() - 1);
    await this.dialect.nakMessage(queue.name, messageId, receiptHandle, expiresAt, reason);
  }

  public async deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void> {
    const queue = await this.getCachedQueueInfo(queueName);
    if (receiptHandle) {
      await this.dialect.deleteMessageByMessageIdAndReceiptHandle(queue.name, messageId, receiptHandle);
    } else {
      await this.dialect.deleteMessageByMessageId(queue.name, messageId);
    }
  }

  public async deleteQueue(queueName: string): Promise<void> {
    const queue = await this.getCachedQueueInfo(queueName);
    const dependentQueues = await this.dialect.findQueueNamesWithDeadLetterQueueName(queueName);
    if (dependentQueues.length > 0) {
      throw new DeleteDeadLetterQueueError(dependentQueues[0], queue.name);
    }
    delete this.cachedQueueInfo[queue.name];
    await this.dialect.deleteQueue(queue.name);
  }

  public async purgeQueue(queueName: string): Promise<void> {
    await this.dialect.purgeQueue(queueName);
  }

  public getTopicInfo(topicName: string): Promise<TopicInfo> {
    return this.getTopicRequired(topicName);
  }

  public getTopicInfos(): Promise<TopicInfo[]> {
    return this.dialect.getTopicInfos();
  }

  public async createTopic(topicName: string, options?: CreateTopicOptions): Promise<void> {
    const requiredOptions: CreateTopicOptions = { ...options };
    if (requiredOptions.tags === undefined) {
      requiredOptions.tags = {};
    }

    const existingTopic = await this.dialect.getTopicInfo(topicName);
    if (existingTopic) {
      if (!topicInfoEqualCreateTopicOptions(existingTopic, requiredOptions)) {
        throw new TopicAlreadyExistsError(topicName);
      }
    }

    await this.dialect.createTopic(topicName, options);
  }

  public subscribe(topicName: string, protocol: TopicProtocol, target: string): Promise<string> {
    switch (protocol) {
      case TopicProtocol.Queue:
        return this.subscribeQueue(topicName, target);
      default:
        throw new Error(`unexpected topic protocol: ${protocol}`);
    }
  }

  public async subscribeQueue(topicName: string, queueName: string): Promise<string> {
    const id = createId();
    const topic = await this.getTopicRequired(topicName);
    const queue = await this.getQueueRequired(queueName);
    await this.dialect.subscribe(id, topic.name, queue.name);
    return id;
  }

  private async getQueueRequired(queueName: string): Promise<QueueInfo> {
    const topic = await this.dialect.getQueueInfo(queueName);
    if (topic) {
      return topic;
    }
    throw new QueueNotFoundError(queueName);
  }

  private async getTopicRequired(topicName: string): Promise<TopicInfo> {
    const topic = await this.dialect.getTopicInfo(topicName);
    if (topic) {
      return topic;
    }
    throw new TopicNotFoundError(topicName);
  }

  public async deleteTopic(topicName: string): Promise<void> {
    const topic = await this.getTopicRequired(topicName);
    await this.dialect.deleteTopic(topic.name);
    delete this.cachedTopicInfo[topicName];
  }
}
