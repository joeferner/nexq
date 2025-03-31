import {
  createId,
  CreateQueueOptions,
  CreateTopicOptions,
  DEFAULT_NAK_EXPIRE_BEHAVIOR,
  GetMessage,
  Message,
  MessageNotFoundError,
  MoveMessagesResult,
  PeekMessagesOptions,
  QueueInfo,
  QueueNotFoundError,
  ReceiptHandleIsInvalidError,
  ReceivedMessage,
  SendMessageOptions,
  Time,
  TopicInfo,
  TopicNotFoundError,
  UpdateMessageOptions,
  User,
} from "@nexq/core";
import * as R from "radash";
import { FindQueueNamesWithDeadLetterQueueNameRow } from "../sql/dto/FindQueueNamesWithDeadLetterQueueNameRow.js";
import { FindQueueNamesWithDeadLetterTopicNameRow } from "../sql/dto/FindQueueNamesWithDeadLetterTopicNameRow.js";
import { RunResult } from "../sql/dto/RunResult.js";
import { SqlCount } from "../sql/dto/SqlCount.js";
import {
  SqlMessage,
  sqlMessageToGetMessage,
  sqlMessageToMessage,
  sqlMessageToReceivedMessage,
} from "../sql/dto/SqlMessage.js";
import { SqlQueue, sqlQueueToQueueInfo } from "../sql/dto/SqlQueue.js";
import { SqlTopicWithSubscription, sqlTopicWithSubscriptionToTopicInfo } from "../sql/dto/SqlTopicWithSubscription.js";
import { SqlUser, sqlUserToUser } from "../sql/dto/SqlUser.js";
import {
  Sql,
  SQL_CREATE_MESSAGE,
  SQL_CREATE_QUEUE,
  SQL_CREATE_SUBSCRIPTION,
  SQL_CREATE_TOPIC,
  SQL_CREATE_USER,
  SQL_DECREASE_PRIORITY_OF_EXPIRED_MESSAGES,
  SQL_DELETE_ALL_MESSAGES,
  SQL_DELETE_ALL_QUEUES,
  SQL_DELETE_ALL_SUBSCRIPTIONS,
  SQL_DELETE_ALL_TOPICS,
  SQL_DELETE_ALL_USERS,
  SQL_DELETE_EXPIRED_MESSAGES_OVER_RECEIVE_COUNT,
  SQL_DELETE_EXPIRED_MESSAGES_OVER_RETENTION,
  SQL_DELETE_MESSAGE_BY_MESSAGE_ID,
  SQL_DELETE_MESSAGE_BY_MESSAGE_ID_AND_RECEIPT_HANDLE,
  SQL_DELETE_MESSAGE_BY_RECEIPT_HANDLE,
  SQL_DELETE_QUEUE,
  SQL_DELETE_TOPIC,
  SQL_FIND_MESSAGE_BY_ID,
  SQL_FIND_MESSAGES_TO_RECEIVE,
  SQL_FIND_QUEUE_BY_NAME,
  SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_QUEUE_NAME,
  SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_TOPIC_NAME,
  SQL_FIND_QUEUES,
  SQL_FIND_TOPIC_INFO_BY_NAME,
  SQL_FIND_TOPIC_INFOS,
  SQL_FIND_USER_BY_ACCESS_KEY_ID,
  SQL_FIND_USER_BY_USERNAME,
  SQL_FIND_USERS,
  SQL_GET_EXPIRED_MESSAGES,
  SQL_GET_QUEUE_NUMBER_OF_DELAYED_MESSAGES,
  SQL_GET_QUEUE_NUMBER_OF_NOT_VISIBLE_MESSAGES,
  SQL_GET_QUEUE_NUMBER_OF_VISIBLE_MESSAGES,
  SQL_GET_SUBSCRIPTION,
  SQL_MOVE_EXPIRED_MESSAGES_TO_DEAD_LETTER,
  SQL_MOVE_EXPIRED_MESSAGES_TO_END_OF_QUEUE,
  SQL_MOVE_MESSAGES,
  SQL_NAK_MESSAGE,
  SQL_PAUSE_QUEUE,
  SQL_PEEK_MESSAGES,
  SQL_PURGE_QUEUE,
  SQL_RECEIVE_MESSAGE,
  SQL_RESUME_QUEUE,
  SQL_UPDATE_MESSAGE_VISIBILITY_BY_RECEIPT_HANDLE,
  SQL_UPDATE_QUEUE,
  SQL_UPDATE_QUEUE_EXPIRES_AT,
} from "../sql/Sql.js";
import { parseOptionalDate } from "../utils.js";
import { DialectCreateUser } from "./dto/DialectCreateUser.js";
import { Transaction } from "./Transaction.js";
import { SqlSubscription } from "../sql/dto/SqlSubscription.js";

export abstract class Dialect<TDatabase, TSql extends Sql<TDatabase>> {
  protected constructor(
    protected readonly sql: TSql,
    protected readonly database: TDatabase,
    protected readonly time: Time
  ) {}

  public abstract beginTransaction(): Promise<Transaction>;

  public async withTransaction<T>(fn: (tx: Transaction) => Promise<T>): Promise<T> {
    const tx = await this.beginTransaction();
    try {
      const result = await fn(tx);
      await tx.commit();
      return result;
    } catch (err) {
      await tx.rollback();
      throw err;
    } finally {
      await tx.release();
    }
  }

  public async findUserByAccessKeyId(tx: Transaction | undefined, accessKeyId: string): Promise<User | undefined> {
    const rows = await this.sql.all<SqlUser>(tx ?? this.database, SQL_FIND_USER_BY_ACCESS_KEY_ID, [accessKeyId]);
    return expect0or1Row(rows, sqlUserToUser);
  }

  public async findUserByUsername(tx: Transaction | undefined, username: string): Promise<User | undefined> {
    const rows = await this.sql.all<SqlUser>(tx ?? this.database, SQL_FIND_USER_BY_USERNAME, [username]);
    return expect0or1Row(rows, sqlUserToUser);
  }

  public async createUser(tx: Transaction | undefined, options: DialectCreateUser): Promise<void> {
    await this.sql.run(tx ?? this.database, SQL_CREATE_USER, [
      options.id,
      options.username,
      options.passwordHash,
      options.accessKeyId,
      options.secretAccessKey,
    ]);
  }

  public async findUsers(): Promise<User[]> {
    const rows = await this.sql.all<SqlUser>(this.database, SQL_FIND_USERS, []);
    return rows.map(sqlUserToUser);
  }

  public async updateQueueExpiresAt(queueName: string, newExpires: Date): Promise<void> {
    await this.sql.run(this.database, SQL_UPDATE_QUEUE_EXPIRES_AT, [newExpires, queueName]);
  }

  public async sendMessage(
    tx: Transaction | undefined,
    queueInfo: QueueInfo,
    id: string,
    body: string,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): Promise<void> {
    const now = this.time.getCurrentTime();
    const retainUntil =
      queueInfo.messageRetentionPeriodMs === undefined
        ? null
        : new Date(now.getTime() + queueInfo.messageRetentionPeriodMs);
    await this.sql.run(tx ?? this.database, SQL_CREATE_MESSAGE, [
      id,
      queueInfo.name,
      options?.priority ?? 0,
      now, // sent at
      now, // order by
      retainUntil,
      body,
      0, // receive count
      options?.attributes ? JSON.stringify(options.attributes) : "{}",
      null, // expires at
      options?.delayMs ? new Date(now.getTime() + options.delayMs) : null,
      options?.lastNakReason ?? null,
    ]);
  }

  public async updateMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): Promise<void> {
    const now = this.time.getCurrentTime();
    const results = await this.sql.updateMessage(this.database, {
      queueName,
      messageId,
      receiptHandle,
      updateMessageOptions,
      now,
    });
    if (results.changes !== 1) {
      const messages = await this.sql.all<SqlMessage>(this.database, SQL_FIND_MESSAGE_BY_ID, [queueName, messageId]);
      if (messages.length === 0) {
        throw new MessageNotFoundError(queueName, messageId);
      }
      if (messages.length > 1) {
        throw new Error(`expected 1 message but found ${messages.length}`);
      }
      if (receiptHandle !== undefined && messages[0].receipt_handle !== receiptHandle) {
        throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
      }
      throw new Error("unexpected state, we found the message by id and the receipt handle matched");
    }
  }

  public async nakMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string,
    newExpiresAt: Date,
    reason?: string
  ): Promise<void> {
    const results = await this.sql.run(this.database, SQL_NAK_MESSAGE, [
      newExpiresAt,
      reason,
      queueName,
      messageId,
      receiptHandle,
    ]);
    if (results.changes !== 1) {
      const messages = await this.sql.all(this.database, SQL_FIND_MESSAGE_BY_ID, [queueName, messageId]);
      if (messages.length === 0) {
        throw new MessageNotFoundError(queueName, messageId);
      }

      throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
    }
  }

  public async receiveMessages(
    queueName: string,
    options: { visibilityTimeoutMs: number; count: number; maxReceiveCount?: number }
  ): Promise<ReceivedMessage[]> {
    return await this.withTransaction(async (tx) => {
      const now = this.time.getCurrentTime();
      const rows = await this.sql.all<SqlMessage>(tx, SQL_FIND_MESSAGES_TO_RECEIVE, [
        queueName,
        now,
        now,
        options.maxReceiveCount ?? 1000000, // choose a large number if not set
        options.count,
      ]);

      const results = await Promise.all(
        rows.map(async (row) => {
          const receiptHandle = createId();
          const newExpireTime = new Date(now.getTime() + options.visibilityTimeoutMs);
          const firstReceivedAt = parseOptionalDate(row.first_received_at) ?? now;
          const receiveCount = row.receive_count + 1;
          await this.sql.run(tx, SQL_RECEIVE_MESSAGE, [
            newExpireTime,
            receiptHandle,
            firstReceivedAt,
            receiveCount,
            row.id,
            queueName,
          ]);
          return sqlMessageToReceivedMessage(row, receiptHandle);
        })
      );
      return results;
    });
  }

  public async peekMessages(queueName: string, options: Required<PeekMessagesOptions>): Promise<Message[]> {
    const now = this.time.getCurrentTime();
    const rows = await this.sql.all<SqlMessage>(this.database, SQL_PEEK_MESSAGES, [
      queueName,
      options.includeNotVisible ? this.getWayInTheFutureDate(now) : now,
      options.includeDelayed ? this.getWayInTheFutureDate(now) : now,
      options.maxNumberOfMessages,
    ]);
    return rows.map(sqlMessageToMessage);
  }

  public async getMessage(queueName: string, messageId: string): Promise<GetMessage> {
    const now = this.time.getCurrentTime();
    const messages = await this.sql.all<SqlMessage & { position: number }>(this.database, SQL_FIND_MESSAGE_BY_ID, [
      queueName,
      messageId,
    ]);
    if (messages.length === 0) {
      throw new MessageNotFoundError(queueName, messageId);
    }
    if (messages.length > 1) {
      throw new Error(`expected 1 message but found ${messages.length}`);
    }

    const message = messages[0];
    return sqlMessageToGetMessage(message, message.position - 1, now);
  }

  /**
   * find a date in the future so that we don't have to write cartesian product
   * sql statements for finding messages
   */
  private getWayInTheFutureDate(now: Date): Date {
    return new Date(now.getTime() + 10 * 360 * 24 * 60 * 60 * 1000);
  }

  public async updateMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void> {
    const newExpiresAt = new Date(this.time.getCurrentTime().getTime() + timeMs);
    const results = await this.sql.run(this.database, SQL_UPDATE_MESSAGE_VISIBILITY_BY_RECEIPT_HANDLE, [
      newExpiresAt,
      queueName,
      receiptHandle,
    ]);
    if (results.changes !== 1) {
      throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
    }
  }

  public async deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void> {
    const results = await this.sql.run(this.database, SQL_DELETE_MESSAGE_BY_RECEIPT_HANDLE, [queueName, receiptHandle]);
    if (results.changes !== 1) {
      throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
    }
  }

  public async deleteExpiredMessagesOverRetention(queueName: string, now: Date): Promise<RunResult> {
    return await this.sql.run(this.database, SQL_DELETE_EXPIRED_MESSAGES_OVER_RETENTION, [queueName, now]);
  }

  public async deleteExpiredMessagesOverReceiveCount(
    queueName: string,
    maxReceiveCount: number,
    now: Date
  ): Promise<RunResult> {
    return await this.sql.run(this.database, SQL_DELETE_EXPIRED_MESSAGES_OVER_RECEIVE_COUNT, [
      queueName,
      now,
      maxReceiveCount + 1,
    ]);
  }

  public async getExpiredMessages(tx: Transaction | undefined, queueInfo: QueueInfo): Promise<SqlMessage[]> {
    const now = this.time.getCurrentTime();
    return this.sql.all<SqlMessage>(tx ?? this.database, SQL_GET_EXPIRED_MESSAGES, [
      queueInfo.name,
      now,
      queueInfo.maxReceiveCount,
    ]);
  }

  public async moveExpiredMessagesToDeadLetter(
    tx: Transaction | undefined,
    queueInfo: QueueInfo,
    deadLetterQueueInfo: QueueInfo
  ): Promise<RunResult> {
    if (queueInfo.maxReceiveCount === undefined) {
      throw new Error("queues with a dead letter queue must have a max receive count");
    }
    const now = this.time.getCurrentTime();
    const newExpiresAt =
      deadLetterQueueInfo.expiresMs === undefined ? null : new Date(now.getTime() + deadLetterQueueInfo.expiresMs);
    return await this.sql.run(tx ?? this.database, SQL_MOVE_EXPIRED_MESSAGES_TO_DEAD_LETTER, [
      deadLetterQueueInfo.name,
      newExpiresAt,
      now, // sent at
      now, // order by
      queueInfo.name,
      now, // expires at
      queueInfo.maxReceiveCount,
    ]);
  }

  public async moveExpiredMessagesToEndOfQueue(queueInfo: QueueInfo): Promise<RunResult> {
    const now = this.time.getCurrentTime();
    return await this.sql.run(this.database, SQL_MOVE_EXPIRED_MESSAGES_TO_END_OF_QUEUE, [
      now, // new order by
      queueInfo.name,
      now, // expires at check
    ]);
  }

  public async decreasePriorityOfExpiredMessages(queueInfo: QueueInfo, decreasePriorityBy: number): Promise<RunResult> {
    const now = this.time.getCurrentTime();
    return await this.sql.run(this.database, SQL_DECREASE_PRIORITY_OF_EXPIRED_MESSAGES, [
      decreasePriorityBy,
      queueInfo.name,
      now, // expires at check
    ]);
  }

  public async getQueueInfos(): Promise<QueueInfo[]> {
    const now = this.time.getCurrentTime();
    const rows = await this.sql.all<SqlQueue>(this.database, SQL_FIND_QUEUES, []);
    // TODO optimize this to find all stats with 1 query
    const queueInfos = await Promise.all(
      rows.map(async (row) => {
        const [numberOfMessagesVisible, numberOfMessagesDelayed, numberOfMessagesNotVisible] = await Promise.all([
          this.getNumberOfVisibleMessages(undefined, row.name, now),
          this.getNumberOfDelayedMessages(undefined, row.name, now),
          this.getNumberOfNotVisibleMessages(undefined, row.name, now),
        ]);

        return {
          ...sqlQueueToQueueInfo(row),
          numberOfMessages: numberOfMessagesVisible + numberOfMessagesDelayed + numberOfMessagesNotVisible,
          numberOfMessagesVisible,
          numberOfMessagesDelayed,
          numberOfMessagesNotVisible,
        } satisfies QueueInfo;
      })
    );
    return R.alphabetical(queueInfos, (t) => t.name);
  }

  public async getQueueInfo(tx: Transaction | undefined, queueName: string): Promise<QueueInfo | undefined> {
    const now = this.time.getCurrentTime();
    const rows = await this.sql.all<SqlQueue>(tx ?? this.database, SQL_FIND_QUEUE_BY_NAME, [queueName]);
    if (rows.length === 0) {
      return undefined;
    }
    if (rows.length > 1) {
      throw new Error(`expected 1 row but found ${rows.length} while getting queue info for "${queueName}"`);
    }

    const [numberOfMessagesVisible, numberOfMessagesDelayed, numberOfMessagesNotVisible] = await Promise.all([
      this.getNumberOfVisibleMessages(tx, queueName, now),
      this.getNumberOfDelayedMessages(tx, queueName, now),
      this.getNumberOfNotVisibleMessages(tx, queueName, now),
    ]);

    return {
      ...sqlQueueToQueueInfo(rows[0]),
      numberOfMessages: numberOfMessagesVisible + numberOfMessagesDelayed + numberOfMessagesNotVisible,
      numberOfMessagesVisible,
      numberOfMessagesDelayed,
      numberOfMessagesNotVisible,
    };
  }

  private async getNumberOfVisibleMessages(tx: Transaction | undefined, queueName: string, now: Date): Promise<number> {
    const rows = await this.sql.all<SqlCount>(tx ?? this.database, SQL_GET_QUEUE_NUMBER_OF_VISIBLE_MESSAGES, [
      queueName,
      now,
      now,
    ]);
    return rows[0].count;
  }

  private async getNumberOfDelayedMessages(tx: Transaction | undefined, queueName: string, now: Date): Promise<number> {
    const rows = await this.sql.all<SqlCount>(tx ?? this.database, SQL_GET_QUEUE_NUMBER_OF_DELAYED_MESSAGES, [
      queueName,
      now,
    ]);
    return rows[0].count;
  }

  private async getNumberOfNotVisibleMessages(
    tx: Transaction | undefined,
    queueName: string,
    now: Date
  ): Promise<number> {
    const rows = await this.sql.all<SqlCount>(tx ?? this.database, SQL_GET_QUEUE_NUMBER_OF_NOT_VISIBLE_MESSAGES, [
      queueName,
      now,
    ]);
    return rows[0].count;
  }

  public async createQueue(tx: Transaction, queueName: string, options?: CreateQueueOptions): Promise<void> {
    const now = this.time.getCurrentTime();
    await this.sql.run(tx ?? this.database, SQL_CREATE_QUEUE, [
      queueName,
      options?.deadLetterQueueName ?? null,
      options?.deadLetterTopicName ?? null,
      options?.delayMs ?? null,
      options?.messageRetentionPeriodMs ?? null,
      options?.visibilityTimeoutMs ?? null,
      options?.receiveMessageWaitTimeMs ?? null,
      options?.expiresMs ?? null,
      options?.expiresMs !== undefined ? new Date(now.getTime() + options.expiresMs) : null,
      options?.maxReceiveCount ?? null,
      options?.maxMessageSize ?? null,
      JSON.stringify(options?.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR),
      this.toSqlBoolean(false), // paused
      options?.tags ? JSON.stringify(options.tags) : "{}",
      now, // created at
      now, // last modified
    ]);
  }

  protected abstract toSqlBoolean(v: boolean): unknown;

  public async updateQueue(tx: Transaction, queueName: string, options?: CreateQueueOptions): Promise<void> {
    const now = this.time.getCurrentTime();
    await this.sql.run(tx ?? this.database, SQL_UPDATE_QUEUE, [
      options?.deadLetterQueueName ?? null,
      options?.deadLetterTopicName ?? null,
      options?.delayMs ?? null,
      options?.messageRetentionPeriodMs ?? null,
      options?.visibilityTimeoutMs ?? null,
      options?.receiveMessageWaitTimeMs ?? null,
      options?.expiresMs ?? null,
      options?.expiresMs !== undefined ? new Date(now.getTime() + options.expiresMs) : null,
      options?.maxReceiveCount ?? null,
      options?.maxMessageSize ?? null,
      JSON.stringify(options?.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR),
      options?.tags ? JSON.stringify(options.tags) : "{}",
      now, // last modified
      queueName,
    ]);
  }

  public async pause(queueName: string): Promise<void> {
    await this.sql.run(this.database, SQL_PAUSE_QUEUE, [queueName]);
  }

  public async resume(queueName: string): Promise<void> {
    await this.sql.run(this.database, SQL_RESUME_QUEUE, [queueName]);
  }

  public async deleteQueue(queueName: string): Promise<void> {
    const results = await this.sql.run(this.database, SQL_DELETE_QUEUE, [queueName]);
    if (results.changes !== 1) {
      throw new QueueNotFoundError(queueName);
    }
  }

  public async findQueueNamesWithDeadLetterQueueName(queueName: string): Promise<string[]> {
    const rows = await this.sql.all<FindQueueNamesWithDeadLetterQueueNameRow>(
      this.database,
      SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_QUEUE_NAME,
      [queueName]
    );
    return rows.map((row) => row.name);
  }

  public async findQueueNamesWithDeadLetterTopicName(topicName: string): Promise<string[]> {
    const rows = await this.sql.all<FindQueueNamesWithDeadLetterTopicNameRow>(
      this.database,
      SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_TOPIC_NAME,
      [topicName]
    );
    return rows.map((row) => row.name);
  }

  public async deleteMessageByMessageId(
    tx: Transaction | undefined,
    queueName: string,
    messageId: string
  ): Promise<void> {
    const results = await this.sql.run(tx ?? this.database, SQL_DELETE_MESSAGE_BY_MESSAGE_ID, [queueName, messageId]);
    if (results.changes !== 1) {
      throw new MessageNotFoundError(queueName, messageId);
    }
  }

  public async deleteMessageByMessageIdAndReceiptHandle(
    queueName: string,
    messageId: string,
    receiptHandle: string
  ): Promise<void> {
    const results = await this.sql.run(this.database, SQL_DELETE_MESSAGE_BY_MESSAGE_ID_AND_RECEIPT_HANDLE, [
      queueName,
      messageId,
      receiptHandle,
    ]);
    if (results.changes !== 1) {
      const message = await this.findMessageById(queueName, messageId);
      if (!message) {
        throw new MessageNotFoundError(queueName, messageId);
      }
      if (message.receipt_handle !== receiptHandle) {
        throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
      }
      throw new Error(
        `unexpected state, tried to delete message by id and receipt handle but didn't, but found message with same receipt handle`
      );
    }
  }

  public async findMessageById(queueName: string, messageId: string): Promise<SqlMessage | undefined> {
    const rows = await this.sql.all(this.database, SQL_FIND_MESSAGE_BY_ID, [queueName, messageId]);
    if (rows.length === 0) {
      return undefined;
    }
    if (rows.length > 1) {
      throw new Error(`expected 1 row but found ${rows.length}`);
    }
    return rows[0] as SqlMessage;
  }

  public async purgeQueue(queueName: string): Promise<void> {
    await this.sql.run(this.database, SQL_PURGE_QUEUE, [queueName]);
  }

  public async moveMessages(
    sourceQueueName: string,
    targetQueueName: string,
    retainUntil: Date | undefined
  ): Promise<MoveMessagesResult> {
    const now = this.time.getCurrentTime();
    const results = await this.sql.run(this.database, SQL_MOVE_MESSAGES, [
      targetQueueName,
      retainUntil ?? null,
      sourceQueueName,
      now,
      now,
    ]);
    return { movedMessageCount: results.changes };
  }

  public async getTopicInfos(): Promise<TopicInfo[]> {
    const rows = await this.sql.all<SqlTopicWithSubscription>(this.database, SQL_FIND_TOPIC_INFOS, []);
    const topicRows = Object.values(R.group(rows, (r) => r.name)).filter((r) => !!r);
    const topics = R.alphabetical(topicRows, (t) => t[0].name);
    return topics.map(sqlTopicWithSubscriptionToTopicInfo);
  }

  public async getTopicInfo(tx: Transaction | undefined, topicName: string): Promise<TopicInfo | undefined> {
    const rows = await this.sql.all<SqlTopicWithSubscription>(tx ?? this.database, SQL_FIND_TOPIC_INFO_BY_NAME, [
      topicName,
    ]);
    if (rows.length === 0) {
      return undefined;
    }
    return sqlTopicWithSubscriptionToTopicInfo(rows);
  }

  public async createTopic(
    tx: Transaction | undefined,
    topicName: string,
    options?: CreateTopicOptions
  ): Promise<void> {
    const now = this.time.getCurrentTime();
    await this.sql.run(tx ?? this.database, SQL_CREATE_TOPIC, [
      topicName,
      options?.tags ? JSON.stringify(options.tags) : "{}",
      now,
      now,
    ]);
  }

  public async subscribe(
    tx: Transaction | undefined,
    id: string,
    topicName: string,
    queueName: string
  ): Promise<RunResult> {
    return await this.sql.run(tx ?? this.database, SQL_CREATE_SUBSCRIPTION, [id, topicName, queueName]);
  }

  public async getSubscription(
    tx: Transaction | undefined,
    topicName: string,
    queueName: string
  ): Promise<SqlSubscription | undefined> {
    const results = await this.sql.all<SqlSubscription>(tx ?? this.database, SQL_GET_SUBSCRIPTION, [
      topicName,
      queueName,
    ]);
    if (results.length === 0) {
      return undefined;
    }
    if (results.length === 1) {
      return results[0];
    }
    throw new Error(`expected 0 or 1 rows but found ${results.length}`);
  }

  public async deleteTopic(topicName: string): Promise<void> {
    const results = await this.sql.run(this.database, SQL_DELETE_TOPIC, [topicName]);
    if (results.changes !== 1) {
      throw new TopicNotFoundError(topicName);
    }
  }

  public async deleteAllData(): Promise<void> {
    await this.sql.run(this.database, SQL_DELETE_ALL_MESSAGES, []);
    await this.sql.run(this.database, SQL_DELETE_ALL_SUBSCRIPTIONS, []);
    await this.sql.run(this.database, SQL_DELETE_ALL_TOPICS, []);
    await this.sql.run(this.database, SQL_DELETE_ALL_QUEUES, []);
    await this.sql.run(this.database, SQL_DELETE_ALL_USERS, []);
  }
}

function expect0or1Row<TRow, T>(rows: TRow[], fn: (row: TRow) => T): T | undefined {
  if (rows.length === 0) {
    return undefined;
  } else if (rows.length === 1) {
    return fn(rows[0]);
  } else {
    throw new Error(`expected 0 or 1 row but found ${rows.length}`);
  }
}
