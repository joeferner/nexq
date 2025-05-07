import {
  AbortError,
  createId,
  createLogger,
  CreateQueueOptions,
  CreateTopicOptions,
  CreateUserOptions,
  DEFAULT_PASSWORD_HASH_ROUNDS,
  DEFAULT_VISIBILITY_TIMEOUT_MS,
  DeleteDeadLetterQueueError,
  DeleteDeadLetterTopicError,
  GetMessage,
  hashPassword,
  InvalidQueueNameError,
  InvalidTopicNameError,
  InvalidUpdateError,
  isDecreasePriorityByNakExpireBehavior,
  Message,
  MessageExceededMaxMessageSizeError,
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
  Trigger,
  UpdateMessageOptions,
  User,
  UserAccessKeyIdAlreadyExistsError,
  UsernameAlreadyExistsError,
} from "@nexq/core";
import { SendMessagesOptions } from "@nexq/core/build/dto/SendMessagesOptions.js";
import { SendMessagesResult } from "@nexq/core/build/dto/SendMessagesResult.js";
import { DialectMessageNotification } from "./dialect/Dialect.js";
import { PostgresDialect } from "./dialect/PostgresDialect.js";
import { SqliteDialect } from "./dialect/SqliteDialect.js";
import { Transaction } from "./dialect/Transaction.js";
import { NewQueueMessageEvent, ResumeEvent } from "./events.js";
import { sqlMessageToMessage } from "./sql/dto/SqlMessage.js";
import { clearRecord } from "./utils.js";

const logger = createLogger("SqlStore");

export const DEFAULT_POLL_INTERVAL = "30s";

/**
 * configure expected in nexq.yaml file
 */
export interface _SqlStoreCreateConfig {
  dialect: "sqlite" | "postgres";
  pollInterval?: string;
  connectionString: string;
}

export interface SqlStoreCreateConfigSqlite extends _SqlStoreCreateConfig {
  dialect: "sqlite";
  options?: {
    readonly?: boolean;
    fileMustExist?: boolean;
    timeout?: number;
    nativeBinding?: string;
  };
}

export interface SqlStoreCreateConfigPostgres extends _SqlStoreCreateConfig {
  dialect: "postgres";
  options?: {
    user?: string;
    password?: string;
    keepAlive?: boolean;
    statement_timeout?: false | number;
    ssl?: {
      ca?: string;
      cert?: string;
      key?: string;
    };
    query_timeout?: number;
    lock_timeout?: number;
    keepAliveInitialDelayMillis?: number;
    idle_in_transaction_session_timeout?: number;
    application_name?: string;
    connectionTimeoutMillis?: number;
    options?: string;
  };
}

export type SqlStoreCreateConfig = SqlStoreCreateConfigSqlite | SqlStoreCreateConfigPostgres;

/**
 * configuring expected when creating a SqlStore
 */
export interface SqlStoreCreateOptions {
  time: Time;
  passwordHashRounds?: number;
  initialUsers?: CreateUserOptions[];
  config: SqlStoreCreateConfig;
}

type QueueTriggerMessage = NewQueueMessageEvent | ResumeEvent | DialectMessageNotification;
type QueueTrigger = Trigger<QueueTriggerMessage>;

export class SqlStore implements Store {
  private readonly time: Time;
  private readonly passwordHashRounds: number;
  private readonly pollIntervalMs: number;
  private readonly dialect: SqliteDialect | PostgresDialect;
  private readonly triggers: Record<string, QueueTrigger[]> = {};
  private readonly cachedQueueInfo: Record<string, QueueInfo> = {};
  private readonly cachedTopicInfo: Record<string, TopicInfo> = {};
  private readonly queueTouched: Record<string, Date> = {};
  private pollHandle?: Timeout;

  private constructor(options: SqlStoreCreateOptions & { _dialect: SqliteDialect | PostgresDialect }) {
    this.time = options.time;
    this.dialect = options._dialect;
    this.dialect.on("messageNotification", (notification) => {
      this.handleDialectMessageNotification(notification);
    });
    this.passwordHashRounds = options.passwordHashRounds ?? DEFAULT_PASSWORD_HASH_ROUNDS;

    let pollIntervalMs = parseDurationIntoMs(options.config.pollInterval ?? DEFAULT_POLL_INTERVAL);
    if (pollIntervalMs < 1000) {
      logger.warn(`minimum poll interval is 1s but found ${pollIntervalMs}ms`);
      pollIntervalMs = 1000;
    }
    this.pollIntervalMs = pollIntervalMs;
  }

  public static async create(options: SqlStoreCreateOptions): Promise<SqlStore> {
    const dialect = await this.createDialect(options);
    const store = new SqlStore({
      ...options,
      _dialect: dialect,
    });

    if (options.initialUsers) {
      const existingUsers = await store.getUsers();
      if (existingUsers.length === 0) {
        for (const initialUser of options.initialUsers) {
          await store.createUser(initialUser);
        }
      }
    }
    return store;
  }

  private static createDialect(options: SqlStoreCreateOptions): Promise<SqliteDialect | PostgresDialect> {
    const dialect = options.config.dialect;
    switch (dialect) {
      case "sqlite":
        return SqliteDialect.create(options.config, options.time);
      case "postgres":
        return PostgresDialect.create(options.config, options.time);
      default:
        throw new Error(`unhandled dialect: ${dialect}`);
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
    await this.dialect.withTransaction(async (tx) => {
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
    });
  }

  public getUserByUsername(username: string): Promise<User | undefined> {
    return this.dialect.findUserByUsername(undefined, username);
  }

  public getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined> {
    return this.dialect.findUserByAccessKeyId(undefined, accessKeyId);
  }

  public getUsers(): Promise<User[]> {
    return this.dialect.findUsers();
  }

  public async createQueue(queueName: string, options?: CreateQueueOptions): Promise<void> {
    queueName = queueName.trim();
    if (queueName.length === 0) {
      throw new InvalidQueueNameError(queueName);
    }

    await this.dialect.withTransaction(async (tx) => {
      const existingQueue = await this.dialect.getQueueInfo(tx, queueName);
      if (existingQueue && options?.upsert !== true) {
        const match = queueInfoEqualCreateQueueOptions(existingQueue, options ?? {});
        if (match === true) {
          return;
        }
        throw new QueueAlreadyExistsError(queueName, match.reason);
      }

      if (options?.deadLetterQueueName) {
        const deadLetterQueue = await this.dialect.getQueueInfo(tx, options.deadLetterQueueName);
        if (!deadLetterQueue) {
          throw new QueueNotFoundError(options.deadLetterQueueName);
        }
      }

      if (options?.deadLetterTopicName) {
        const deadLetterTopic = await this.dialect.getTopicInfo(tx, options.deadLetterTopicName);
        if (!deadLetterTopic) {
          throw new TopicNotFoundError(options.deadLetterTopicName);
        }
      }

      if (options?.upsert) {
        await this.dialect.updateQueue(tx, queueName, options);
      } else {
        await this.dialect.createQueue(tx, queueName, options);
      }
    });
  }

  public async sendMessage(queueName: string, body: string, options?: SendMessageOptions): Promise<SendMessageResult> {
    const id = createId();
    const queueInfo = await this.getCachedQueueInfo(queueName);
    if (queueInfo.maxMessageSize && body.length > queueInfo.maxMessageSize) {
      throw new MessageExceededMaxMessageSizeError(body.length, queueInfo.maxMessageSize);
    }
    await this.dialect.sendMessage(undefined, queueInfo, id, body, options);
    this.trigger({ type: "newQueueMessageEvent", queueName } satisfies NewQueueMessageEvent);
    return { id };
  }

  public async sendMessages(queueName: string, options: SendMessagesOptions): Promise<SendMessagesResult> {
    const queueInfo = await this.getCachedQueueInfo(queueName);
    const { maxMessageSize } = queueInfo;
    if (maxMessageSize) {
      const tooLargeMessage = options.messages.find((m) => m.body.length > maxMessageSize);
      if (tooLargeMessage) {
        throw new MessageExceededMaxMessageSizeError(tooLargeMessage.body.length, maxMessageSize);
      }
    }
    const ids = options.messages.map((_) => createId());
    await this.dialect.withTransaction(async (tx) => {
      for (let i = 0; i < options.messages.length; i++) {
        const message = options.messages[i];
        const id = ids[i];
        await this.dialect.sendMessage(tx, queueInfo, id, message.body, message);
      }
    });
    this.trigger({ type: "newQueueMessageEvent", queueName } satisfies NewQueueMessageEvent);
    return { ids };
  }

  public async publishMessage(topicName: string, body: string, options?: SendMessageOptions): Promise<void> {
    const topic = await this.getCachedTopicInfo(topicName);
    await this.dialect.withTransaction(async (tx) => {
      await this.internalPublishMessage(tx, topic, body, options);
    });
  }

  private async internalPublishMessage(
    tx: Transaction,
    topic: TopicInfo,
    body: string,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): Promise<void> {
    const queueInfos = await Promise.all(topic.subscriptions.map((sub) => this.getCachedQueueInfo(sub.queueName)));
    for (const queueInfo of queueInfos) {
      if (queueInfo.maxMessageSize && body.length > queueInfo.maxMessageSize) {
        throw new MessageExceededMaxMessageSizeError(body.length, queueInfo.maxMessageSize);
      }
    }

    for (const queueInfo of queueInfos) {
      const id = createId();
      await this.dialect.sendMessage(tx, queueInfo, id, body, options);
      this.trigger({ type: "newQueueMessageEvent", queueName: queueInfo.name } satisfies NewQueueMessageEvent);
    }
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
    const now = this.time.getCurrentTime();
    const nowMs = now.getTime();
    let queueInfo = await this.getCachedQueueInfo(queueName);
    const visibilityTimeoutMs =
      options?.visibilityTimeoutMs ?? queueInfo.visibilityTimeoutMs ?? DEFAULT_VISIBILITY_TIMEOUT_MS;
    const waitTime = options?.waitTimeMs ?? queueInfo.receiveMessageWaitTimeMs ?? 0;
    const endTime = nowMs + waitTime;
    if (queueInfo.expiresMs) {
      if (!this.queueTouched[queueName] || this.queueTouched[queueName] < now) {
        this.queueTouched[queueName] = now;
      }
    }
    while (true) {
      const trigger = this.addTrigger(queueName);

      if (!queueInfo.paused) {
        const messages = await this.dialect.receiveMessages(queueName, {
          visibilityTimeoutMs,
          count: options?.maxNumberOfMessages ?? 10,
          maxReceiveCount: queueInfo.maxReceiveCount,
        });
        if (messages.length > 0) {
          return messages;
        }
      }

      const now = this.time.getCurrentTime();
      if (now.getTime() > endTime) {
        return [];
      }

      const timeLeft = endTime - now.getTime();
      if (timeLeft > 0) {
        const waitResult = await trigger.waitUntil(new Date(endTime), { signal: options?.abortSignal });
        if (options?.abortSignal?.aborted) {
          throw new AbortError("receive aborted");
        }
        if (waitResult !== "timeout" && queueInfo.paused) {
          queueInfo = await this.getQueueInfo(queueName);
        }
      } else {
        return [];
      }
    }
  }

  public async peekMessages(queueName: string, options?: PeekMessagesOptions): Promise<Message[]> {
    const queue = await this.getCachedQueueInfo(queueName);
    return this.dialect.peekMessages(queue.name, toRequiredPeekMessagesOptions(options));
  }

  public async getMessage(queueName: string, messageId: string): Promise<GetMessage> {
    const queue = await this.getCachedQueueInfo(queueName);
    return this.dialect.getMessage(queue.name, messageId);
  }

  private handleDialectMessageNotification(notification: DialectMessageNotification): void {
    this.trigger(notification);
  }

  private addTrigger(queueName: string): QueueTrigger {
    const trigger = new Trigger<QueueTriggerMessage>(this.time);
    this.triggers[queueName] ??= [];
    this.triggers[queueName].push(trigger);
    return trigger;
  }

  private trigger(message: QueueTriggerMessage): void {
    const queueName = message.queueName;
    if (!(queueName in this.triggers)) {
      return;
    }
    const triggers = this.triggers[queueName];
    delete this.triggers[queueName];

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

  public async pause(queueName: string): Promise<void> {
    const queue = await this.getCachedQueueInfo(queueName);
    queue.paused = true;
    await this.dialect.pause(queueName);
  }

  public async resume(queueName: string): Promise<void> {
    await this.dialect.resume(queueName);
    const queue = await this.getCachedQueueInfo(queueName);
    queue.paused = false;
    this.trigger({ type: "resumeEvent", queueName } satisfies ResumeEvent);
  }

  public async poll(): Promise<void> {
    const reloadQueueInfosCache = async (): Promise<QueueInfo[]> => {
      const queueInfos = await this.dialect.getQueueInfos();

      // need to update the cachedQueueInfo synchronously to avoid conflicts
      clearRecord(this.cachedQueueInfo);
      for (const queueInfo of queueInfos) {
        this.cachedQueueInfo[queueInfo.name] = queueInfo;
      }

      // update expires at using queueTouched
      for (const queueInfo of queueInfos) {
        if (queueInfo.expiresMs && this.queueTouched[queueInfo.name]) {
          const lastTouched = this.queueTouched[queueInfo.name];
          const newExpires = new Date(lastTouched.getTime() + queueInfo.expiresMs);
          queueInfo.expiresAt = newExpires;
          logger.debug(`updating queue expires at "${queueInfo.name}" = ${newExpires.toISOString()}`);
          await this.dialect.updateQueueExpiresAt(queueInfo.name, newExpires);

          // check that queueTouched didn't update while we were updating the expires at
          if (lastTouched === this.queueTouched[queueInfo.name]) {
            delete this.queueTouched[queueInfo.name];
          }
        }
      }

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
          logger.info(
            `deleting expired queue "${queueInfo.name}" (queue expired at: ${queueInfo.expiresAt.toISOString()}, now: ${now.toISOString()})`
          );
          try {
            await this.deleteQueue(queueInfo.name);
            queueInfos = queueInfos.filter((qi) => qi !== queueInfo);
          } catch (err) {
            if (err instanceof QueueNotFoundError) {
              continue;
            }
            logger.error(`failed to delete queue: ${queueInfo.name}`, err);
          }
        }
      }

      for (const queueInfo of queueInfos) {
        try {
          await this.pollQueue(queueInfo);
        } catch (err) {
          logger.error(`failed to poll queue: ${queueInfo.name}`, err);
        }
      }
    } finally {
      if (!this.pollHandle && this.pollIntervalMs > 0) {
        this.pollHandle = this.time.setTimeout(() => {
          this.pollHandle = undefined;
          void this.poll();
        }, this.pollIntervalMs);
      }
    }
  }

  private async pollQueue(queueInfo: QueueInfo): Promise<void> {
    if (queueInfo.deadLetterTopicName) {
      await this.pollQueueWithDeadLetterTopicAndPossibleQueue(queueInfo);
    } else if (queueInfo.deadLetterQueueName) {
      await this.pollQueueWithOnlyDeadLetterQueue(queueInfo);
    } else {
      await this.pollQueueWithoutDeadLetter(queueInfo);
    }
  }

  private async pollQueueWithoutDeadLetter(queueInfo: QueueInfo): Promise<void> {
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

    if (queueInfo.nakExpireBehavior === "retry") {
      // do nothing
    } else if (queueInfo.nakExpireBehavior === "moveToEnd") {
      const results = await this.dialect.moveExpiredMessagesToEndOfQueue(queueInfo);
      if (results.changes > 0) {
        logger.debug(`moved ${results.changes} messages from "${queueInfo.name}" to end of queue`);
      }
    } else if (isDecreasePriorityByNakExpireBehavior(queueInfo.nakExpireBehavior)) {
      const results = await this.dialect.decreasePriorityOfExpiredMessages(
        queueInfo,
        queueInfo.nakExpireBehavior.decreasePriorityBy
      );
      if (results.changes > 0) {
        logger.debug(
          `decrease the priority of ${results.changes} messages in "${queueInfo.name}" by ${queueInfo.nakExpireBehavior.decreasePriorityBy}`
        );
      }
    } else {
      throw new Error(`unhandled nakExpireBehavior "${JSON.stringify(queueInfo.nakExpireBehavior)}"`);
    }
  }

  private async pollQueueWithOnlyDeadLetterQueue(queueInfo: QueueInfo): Promise<void> {
    if (!queueInfo.deadLetterQueueName) {
      throw new Error(`missing dead letter queue name`);
    }

    // we can avoid a transaction here since we are executing a single sql statement
    const deadLetterQueueInfo = await this.getCachedQueueInfo(queueInfo.deadLetterQueueName);
    const results = await this.dialect.moveExpiredMessagesToDeadLetter(undefined, queueInfo, deadLetterQueueInfo);
    if (results.changes > 0) {
      logger.debug(
        `moved ${results.changes} messages from "${queueInfo.name}" to dead letter "${queueInfo.deadLetterQueueName}"`
      );
    }
  }

  private async pollQueueWithDeadLetterTopicAndPossibleQueue(queueInfo: QueueInfo): Promise<void> {
    await this.dialect.withTransaction(async (tx) => {
      const expiredSqlMessages = await this.dialect.getExpiredMessages(tx, queueInfo);

      for (const sqlMessage of expiredSqlMessages) {
        const message = sqlMessageToMessage(sqlMessage);
        const options: SendMessageOptions & { lastNakReason?: string } = {
          priority: message.priority,
          delayMs: 0,
          attributes: message.attributes,
          lastNakReason: message.lastNakReason,
        };

        if (queueInfo.deadLetterTopicName) {
          const deadLetterTopicInfo = await this.getCachedTopicInfo(queueInfo.deadLetterTopicName);
          await this.internalPublishMessage(tx, deadLetterTopicInfo, message.body, options);
        }

        if (queueInfo.deadLetterQueueName) {
          const deadLetterQueueInfo = await this.getCachedQueueInfo(queueInfo.deadLetterQueueName);
          await this.dialect.sendMessage(tx, deadLetterQueueInfo, sqlMessage.id, message.body, options);
        }

        await this.dialect.deleteMessageByMessageId(tx, queueInfo.name, sqlMessage.id);
      }

      const to: string[] = [];
      if (queueInfo.deadLetterTopicName) {
        to.push(`dead letter topic "${queueInfo.deadLetterTopicName}"`);
      }
      if (queueInfo.deadLetterQueueName) {
        to.push(`dead letter queue "${queueInfo.deadLetterQueueName}"`);
      }
      logger.debug(`moved ${expiredSqlMessages.length} messages from "${queueInfo.name}" to ${to.join(",")}`);
    });
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
      await this.dialect.deleteMessageByMessageId(undefined, queue.name, messageId);
    }
  }

  public async deleteQueue(queueName: string): Promise<void> {
    logger.info(`deleting queue: "${queueName}"`);
    const queue = await this.getCachedQueueInfo(queueName);
    const dependentQueues = await this.dialect.findQueueNamesWithDeadLetterQueueName(queueName);
    if (dependentQueues.length > 0) {
      throw new DeleteDeadLetterQueueError(dependentQueues[0], queue.name);
    }
    delete this.cachedQueueInfo[queue.name];
    await this.dialect.deleteQueue(queue.name);
  }

  public async purgeQueue(queueName: string): Promise<void> {
    logger.info(`purging queue: ${queueName}`);
    await this.dialect.purgeQueue(queueName);
  }

  public async moveMessages(sourceQueueName: string, targetQueueName: string): Promise<MoveMessagesResult> {
    const now = this.time.getCurrentTime();
    const sourceQueue = await this.getCachedQueueInfo(sourceQueueName);
    const targetQueue = await this.getCachedQueueInfo(targetQueueName);
    const retainUntil =
      targetQueue.messageRetentionPeriodMs === undefined
        ? undefined
        : new Date(now.getTime() + targetQueue.messageRetentionPeriodMs);
    return await this.dialect.moveMessages(sourceQueue.name, targetQueue.name, retainUntil);
  }

  public getTopicInfo(topicName: string): Promise<TopicInfo> {
    return this.getTopicRequired(topicName);
  }

  public getTopicInfos(): Promise<TopicInfo[]> {
    return this.dialect.getTopicInfos();
  }

  public async createTopic(topicName: string, options?: CreateTopicOptions): Promise<void> {
    topicName = topicName.trim();
    if (topicName.length === 0) {
      throw new InvalidTopicNameError(topicName);
    }

    await this.dialect.withTransaction(async (tx) => {
      const requiredOptions: CreateTopicOptions = { ...options };
      if (requiredOptions.tags === undefined) {
        requiredOptions.tags = {};
      }

      const existingTopic = await this.dialect.getTopicInfo(tx, topicName);
      if (existingTopic) {
        const m = topicInfoEqualCreateTopicOptions(existingTopic, requiredOptions);
        if (m === true) {
          return;
        }
        throw new TopicAlreadyExistsError(topicName, m.reason);
      }

      await this.dialect.createTopic(tx, topicName, options);
    });
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
    return await this.dialect.withTransaction(async (tx) => {
      const id = createId();
      const topic = await this.getTopicRequired(topicName, tx);
      const queue = await this.getQueueRequired(queueName, tx);
      const result = await this.dialect.subscribe(tx, id, topic.name, queue.name);
      if (result.changes === 0) {
        const existingSubscription = await this.dialect.getSubscription(tx, topicName, queueName);
        if (existingSubscription) {
          return existingSubscription.id;
        }
      }
      return id;
    });
  }

  private async getQueueRequired(queueName: string, tx?: Transaction): Promise<QueueInfo> {
    const queue = await this.dialect.getQueueInfo(tx, queueName);
    if (queue) {
      this.cachedQueueInfo[queueName] = queue;
      return queue;
    }
    throw new QueueNotFoundError(queueName);
  }

  private async getTopicRequired(topicName: string, tx?: Transaction): Promise<TopicInfo> {
    const topic = await this.dialect.getTopicInfo(tx, topicName);
    if (topic) {
      return topic;
    }
    throw new TopicNotFoundError(topicName);
  }

  public async deleteTopic(topicName: string): Promise<void> {
    const topic = await this.getTopicRequired(topicName);
    const dependentQueues = await this.dialect.findQueueNamesWithDeadLetterTopicName(topicName);
    if (dependentQueues.length > 0) {
      throw new DeleteDeadLetterTopicError(dependentQueues[0], topic.name);
    }
    await this.dialect.deleteTopic(topic.name);
    delete this.cachedTopicInfo[topicName];
  }

  public async deleteAllData(): Promise<void> {
    await this.dialect.deleteAllData();
  }

  public static maskPasswordInConnectionString(connectionString: string): string {
    connectionString = connectionString.replace(/\/\/([^/:]+?):(.+?)@/, (_m, a) => `//${a}:***@`);
    connectionString = connectionString.replace(/(password)=([^&;]+)/i, (_m, a, _password) => `${a}=***`);
    return connectionString;
  }
}
