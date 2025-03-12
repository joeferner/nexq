import {
  createId,
  CreateQueueOptions,
  CreateTopicOptions,
  CreateUserOptions,
  DEFAULT_PASSWORD_HASH_ROUNDS,
  hashPassword,
  Message,
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
import { Dialect, Transaction } from "./dialect/Dialect.js";
import { SqliteDialect } from "./dialect/SqliteDialect.js";
import { NewQueueMessageEvent } from "./events.js";

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
  private readonly dialect: Dialect;
  private readonly triggers: Trigger<NewQueueMessageEvent>[] = [];
  private readonly cachedQueueInfo: Record<string, QueueInfo> = {};
  private pollHandle?: Timeout;

  private constructor(options: SqlStoreCreateOptions & { _dialect: Dialect }) {
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

  private static createDialect(options: SqlStoreCreateOptions): Promise<Dialect> {
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
    const tx = await this.dialect.createTransaction();
    try {
      const existingUser = await this.dialect.findUserByUsername(tx, options.username);
      if (existingUser) {
        throw new UsernameAlreadyExistsError(options.username);
      }

      if (options.accessKeyId) {
        const existingUserWithAccessKey = await this.dialect.findUserByAccessKeyId(tx, options.accessKeyId);
        if (existingUserWithAccessKey) {
          throw new UserAccessKeyIdAlreadyExistsError(options.accessKeyId);
        }
      }

      await this.dialect.createUser(tx, {
        id: createId(),
        username: options.username,
        passwordHash: options.password ? await hashPassword(options.password, this.passwordHashRounds) : null,
        accessKeyId: options.accessKeyId ?? null,
        secretAccessKey: options.secretAccessKey ?? null,
      });

      await tx.commit();
    } catch (err) {
      await tx.rollback();
      throw err;
    }
  }

  public getUserByUsername(username: string): Promise<User | undefined> {
    return this.dialect.findUserByUsername(undefined, username);
  }

  public getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined> {
    return this.dialect.findUserByAccessKeyId(undefined, accessKeyId);
  }

  public getUsers(): Promise<User[]> {
    return this.dialect.findUsers(undefined);
  }

  public async createQueue(queueName: string, options?: CreateQueueOptions): Promise<void> {
    const tx = await this.dialect.createTransaction();
    try {
      const existingQueue = await this.dialect.getQueueInfo(tx, queueName);
      if (existingQueue) {
        if (queueInfoEqualCreateQueueOptions(existingQueue, options ?? {})) {
          return;
        } else {
          throw new QueueAlreadyExistsError(queueName);
        }
      }

      if (options?.deadLetterQueueName) {
        const deadLetterQueue = await this.dialect.getQueueInfo(tx, options.deadLetterQueueName);
        if (!deadLetterQueue) {
          throw new QueueNotFoundError(options.deadLetterQueueName);
        }
      }

      await this.dialect.createQueue(tx, queueName, options);

      await tx.commit();
    } catch (err) {
      await tx.rollback();
      throw err;
    }
  }

  public async sendMessage(
    queueName: string,
    body: string | Buffer,
    options?: SendMessageOptions
  ): Promise<SendMessageResult> {
    if (R.isString(body)) {
      body = Buffer.from(body);
    }
    const queueInfo = await this.getCachedQueueInfo(queueName);
    const result = this.dialect.sendMessage(queueInfo, body, options);
    console.log("trigger");
    this.trigger({ type: "new-queue-message", queueName } satisfies NewQueueMessageEvent);
    return result;
  }

  public async receiveMessage(queueName: string, options?: ReceiveMessageOptions): Promise<Message | undefined> {
    const messages = await this.receiveMessages(queueName, { ...options, maxNumberOfMessages: 1 });
    if (messages.length > 1) {
      throw new Error(`expected 0 or 1 but found ${messages.length} when receiving messages`);
    }
    return messages[0];
  }

  public async receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<Message[]> {
    const now = this.time.getCurrentTime().getTime();
    const queueInfo = await this.getCachedQueueInfo(queueName);
    const visibilityTimeoutMs = options?.visibilityTimeoutMs ?? queueInfo.visibilityTimeoutMs ?? 60;
    const waitTime = options?.waitTimeMs ?? queueInfo.receiveMessageWaitTimeMs ?? 0;
    const endTime = now + waitTime;
    while (true) {
      const trigger = new Trigger<NewQueueMessageEvent>(this.time);
      this.triggers.push(trigger);

      console.log("receive");
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

      console.log("wait");
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

  public async poll(): Promise<void> {
    try {
      const queueInfos = await this.dialect.getQueueInfos(undefined);
      for (const queueInfo of queueInfos) {
        this.cachedQueueInfo[queueInfo.name] = queueInfo;
      }

      // TODO delete expired queues

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
      await this.dialect.moveExpiredMessagesToDeadLetter(queueInfo, deadLetterQueueInfo);
    } else {
      await this.dialect.deleteExpiredMessages(queueInfo);
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
    return this.getQueueRequired(undefined, queueName);
  }

  public getQueueInfos(): Promise<QueueInfo[]> {
    return this.dialect.getQueueInfos(undefined);
  }

  public updateMessage(
    _queueName: string,
    _messageId: string,
    _receiptHandle: string | undefined,
    _updateMessageOptions: UpdateMessageOptions
  ): Promise<void> {
    throw new Error("Method not implemented: updateMessage");
  }

  public nakMessage(_queueName: string, _messageId: string, _receiptHandle: string): Promise<void> {
    throw new Error("Method not implemented: nakMessage");
  }

  public deleteMessage(_queueName: string, _messageId: string, _receiptHandle?: string): Promise<void> {
    throw new Error("Method not implemented: deleteMessage");
  }

  public deleteQueue(_queueName: string): Promise<void> {
    throw new Error("Method not implemented: deleteQueue");
  }

  public purgeQueue(_queueName: string): Promise<void> {
    throw new Error("Method not implemented: purgeQueue");
  }

  public getTopicInfo(topicName: string): Promise<TopicInfo> {
    return this.getTopicRequired(undefined, topicName);
  }

  public getTopicInfos(): Promise<TopicInfo[]> {
    return this.dialect.getTopicInfos(undefined);
  }

  public async createTopic(topicName: string, options?: CreateTopicOptions): Promise<void> {
    const tx = await this.dialect.createTransaction();
    try {
      const requiredOptions: CreateTopicOptions = { ...options };
      if (requiredOptions.tags === undefined) {
        requiredOptions.tags = {};
      }

      const existingTopic = await this.dialect.getTopicInfo(tx, topicName);
      if (existingTopic) {
        if (!topicInfoEqualCreateTopicOptions(existingTopic, requiredOptions)) {
          throw new TopicAlreadyExistsError(topicName);
        }
      }

      await this.dialect.createTopic(tx, topicName, options);

      await tx.commit();
    } catch (err) {
      await tx.rollback();
      throw err;
    }
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
    const tx = await this.dialect.createTransaction();
    try {
      const id = createId();
      const topic = await this.getTopicRequired(tx, topicName);
      const queue = await this.getQueueRequired(tx, queueName);
      await this.dialect.subscribe(tx, id, topic.name, queue.name);

      await tx.commit();

      return id;
    } catch (err) {
      await tx.rollback();
      throw err;
    }
  }

  private async getQueueRequired(tx: Transaction | undefined, queueName: string): Promise<QueueInfo> {
    const topic = await this.dialect.getQueueInfo(tx, queueName);
    if (topic) {
      return topic;
    }
    throw new QueueNotFoundError(queueName);
  }

  private async getTopicRequired(tx: Transaction | undefined, topicName: string): Promise<TopicInfo> {
    const topic = await this.dialect.getTopicInfo(tx, topicName);
    if (topic) {
      return topic;
    }
    throw new TopicNotFoundError(topicName);
  }

  public deleteTopic(_topicName: string): Promise<void> {
    throw new Error("Method not implemented: deleteTopic");
  }

  public publishMessage(_topicName: string, _body: string, _options?: SendMessageOptions): Promise<SendMessageResult> {
    throw new Error("Method not implemented: publishMessage");
  }
}
