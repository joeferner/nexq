import { UpdateMessageOptions } from "@nexq/core";
import { RunResult } from "./dto/RunResult.js";

export const SQL_DELETE_QUEUE = "deleteQueue";
export const SQL_PURGE_QUEUE = "purgeQueue";
export const SQL_MOVE_MESSAGES = "moveMessages";
export const SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_QUEUE_NAME = "findQueueNamesWithDeadLetterQueueName";
export const SQL_DELETE_MESSAGE_BY_MESSAGE_ID = "deleteMessageByMessageId";
export const SQL_DELETE_MESSAGE_BY_MESSAGE_ID_AND_RECEIPT_HANDLE = "deleteMessageByMessageIdAndReceiptHandle";
export const SQL_FIND_MESSAGE_BY_ID = "findMessageById";
export const SQL_FIND_USER_BY_ACCESS_KEY_ID = "findUserByAccessKeyId";
export const SQL_FIND_USER_BY_USERNAME = "findUserByUsername";
export const SQL_CREATE_USER = "createUser";
export const SQL_FIND_USERS = "findUsers";
export const SQL_CREATE_MESSAGE = "createMessage";
export const SQL_FIND_MESSAGES_TO_RECEIVE = "findMessagesToReceive";
export const SQL_RECEIVE_MESSAGE = "receiveMessage";
export const SQL_UPDATE_MESSAGE_VISIBILITY_BY_RECEIPT_HANDLE = "updateMessageVisibilityByReceiptHandle";
export const SQL_DELETE_MESSAGE_BY_RECEIPT_HANDLE = "deleteMessageByReceiptHandle";
export const SQL_DELETE_EXPIRED_MESSAGES_OVER_RETENTION = "deleteExpiredMessagesOverRetention";
export const SQL_DELETE_EXPIRED_MESSAGES_OVER_RECEIVE_COUNT = "deleteExpiredMessagesOverReceiveCount";
export const SQL_MOVE_EXPIRED_MESSAGES_TO_DEAD_LETTER = "moveExpiredMessagesToDeadLetter";
export const SQL_FIND_QUEUES = "findQueues";
export const SQL_FIND_QUEUE_BY_NAME = "findQueueByName";
export const SQL_GET_QUEUE_NUMBER_OF_MESSAGES = "getQueueNumberOfMessages";
export const SQL_GET_QUEUE_NUMBER_OF_DELAYED_MESSAGES = "getQueueNumberOfDelayedMessage";
export const SQL_GET_QUEUE_NUMBER_OF_NOT_VISIBLE_MESSAGES = "getNumberOfNotVisibleMessages";
export const SQL_CREATE_QUEUE = "createQueue";
export const SQL_FIND_TOPIC_INFOS = "findTopicInfos";
export const SQL_FIND_TOPIC_INFO_BY_NAME = "findTopicInfoByName";
export const SQL_CREATE_TOPIC = "createTopic";
export const SQL_CREATE_SUBSCRIPTION = "createSubscription";
export const SQL_DELETE_TOPIC = "deleteTopic";
export const SQL_NAK_MESSAGE = "nakMessage";
export const SQL_UPDATE_QUEUE_EXPIRES_AT = "updateQueueExpiresAt";
export const SQL_DELETE_ALL_MESSAGES = "deleteAllMessages";
export const SQL_DELETE_ALL_SUBSCRIPTIONS = "deleteAllSubscriptions";
export const SQL_DELETE_ALL_TOPICS = "deleteAllTopics";
export const SQL_DELETE_ALL_QUEUES = "deleteAllQueues";
export const SQL_DELETE_ALL_USERS = "deleteAllUsers";

export abstract class Sql<TDatabase> {
  private readonly queries: Record<string, string> = {};
  protected readonly tablePrefix: string;

  protected constructor() {
    this.tablePrefix = "nexq";

    this.addQuery(
      SQL_DELETE_QUEUE,
      `
        DELETE FROM
          ${this.tablePrefix}_queue
        WHERE
          name = ?
      `
    );

    this.addQuery(
      SQL_PURGE_QUEUE,
      `
        DELETE FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
      `
    );

    this.addQuery(
      SQL_FIND_QUEUE_NAMES_WITH_DEAD_LETTER_QUEUE_NAME,
      `
        SELECT
          name
        FROM
          ${this.tablePrefix}_queue
        WHERE
          dead_letter_queue_name = ?
      `
    );

    this.addQuery(
      SQL_DELETE_MESSAGE_BY_MESSAGE_ID,
      `
        DELETE FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND id = ?
      `
    );

    this.addQuery(
      SQL_DELETE_MESSAGE_BY_MESSAGE_ID_AND_RECEIPT_HANDLE,
      `
        DELETE FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND id = ?
          AND receipt_handle = ?
      `
    );

    this.addQuery(
      SQL_FIND_MESSAGE_BY_ID,
      `
        SELECT
          *
        FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND id = ?
      `
    );

    this.addQuery(
      SQL_FIND_USER_BY_ACCESS_KEY_ID,
      `
        SELECT
          *
        FROM
          ${this.tablePrefix}_user
        WHERE
          access_key_id = ?
      `
    );

    this.addQuery(
      SQL_FIND_USER_BY_USERNAME,
      `
        SELECT
          *
        FROM
          ${this.tablePrefix}_user
        WHERE
          username = ?
      `
    );

    this.addQuery(
      SQL_CREATE_USER,
      `
        INSERT INTO ${this.tablePrefix}_user (
          id,
          username,
          password_hash,
          access_key_id,
          secret_access_key
        ) VALUES (?, ?, ?, ?, ?)
      `
    );

    this.addQuery(
      SQL_FIND_USERS,
      `
        SELECT
          *
        FROM
          ${this.tablePrefix}_user
      `
    );

    this.addQuery(
      SQL_CREATE_MESSAGE,
      `
        INSERT INTO ${this.tablePrefix}_message (
          id,
          queue_name,
          priority,
          sent_at,
          retain_until,
          message_body,
          receive_count,
          attributes,
          expires_at,
          delay_until
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `
    );

    this.addQuery(
      SQL_FIND_MESSAGES_TO_RECEIVE,
      `
        SELECT
          *
        FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND (expires_at IS NULL OR ? > expires_at)
          AND (delay_until IS NULL OR ? >= delay_until)
        ORDER BY
          priority DESC,
          sent_at
        LIMIT ?
      `
    );

    this.addQuery(
      SQL_MOVE_MESSAGES,
      `
        UPDATE
          ${this.tablePrefix}_message
        SET
          queue_name = ?,
          retain_until = ?
        WHERE
          queue_name = ?
          AND (expires_at IS NULL OR ? > expires_at)
          AND (delay_until IS NULL OR ? >= delay_until)
      `
    );

    this.addQuery(
      SQL_RECEIVE_MESSAGE,
      `
        UPDATE
          ${this.tablePrefix}_message
        SET
          expires_at = ?,
          receipt_handle = ?,
          first_received_at = ?,
          receive_count = ?
        WHERE
          id = ?
          AND queue_name = ?
      `
    );

    this.addQuery(
      SQL_UPDATE_MESSAGE_VISIBILITY_BY_RECEIPT_HANDLE,
      `
        UPDATE
          ${this.tablePrefix}_message
        SET
          expires_at = ?
        WHERE
          queue_name = ?
          AND receipt_handle = ?
      `
    );

    this.addQuery(
      SQL_DELETE_MESSAGE_BY_RECEIPT_HANDLE,
      `
        DELETE FROM 
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND receipt_handle = ?
      `
    );

    this.addQuery(
      SQL_DELETE_EXPIRED_MESSAGES_OVER_RETENTION,
      `
        DELETE FROM 
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND (retain_until IS NOT NULL AND ? > retain_until)
      `
    );

    this.addQuery(
      SQL_DELETE_EXPIRED_MESSAGES_OVER_RECEIVE_COUNT,
      `
        DELETE FROM 
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND (expires_at IS NOT NULL AND ? > expires_at)
          OR receive_count >= ?
      `
    );

    this.addQuery(
      SQL_MOVE_EXPIRED_MESSAGES_TO_DEAD_LETTER,
      `
        UPDATE 
          ${this.tablePrefix}_message
        SET
          queue_name = ?,
          expires_at = ?,
          receive_count = 0,
          receipt_handle = null,
          sent_at = ?
        WHERE
          queue_name = ?
          AND (expires_at IS NOT NULL AND ? > expires_at)
          AND receive_count >= ?
      `
    );

    this.addQuery(
      SQL_FIND_QUEUES,
      `
        SELECT
          *
        FROM 
          ${this.tablePrefix}_queue
      `
    );

    this.addQuery(
      SQL_FIND_QUEUE_BY_NAME,
      `
        SELECT
          *
        FROM 
          ${this.tablePrefix}_queue
        WHERE
          name = ?
      `
    );

    this.addQuery(
      SQL_GET_QUEUE_NUMBER_OF_MESSAGES,
      `
        SELECT
          COUNT(*) as count
        FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND (expires_at IS NULL OR expires_at < ?)
          AND (delay_until IS NULL OR ? >= delay_until)
      `
    );

    this.addQuery(
      SQL_GET_QUEUE_NUMBER_OF_DELAYED_MESSAGES,
      `
        SELECT
          COUNT(*) as count
        FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND delay_until IS NOT NULL
          AND ? < delay_until
      `
    );

    this.addQuery(
      SQL_GET_QUEUE_NUMBER_OF_NOT_VISIBLE_MESSAGES,
      `
        SELECT
          COUNT(*) as count
        FROM
          ${this.tablePrefix}_message
        WHERE
          queue_name = ?
          AND expires_at IS NOT NULL
          AND expires_at >= ?
      `
    );

    this.addQuery(
      SQL_CREATE_QUEUE,
      `
        INSERT INTO ${this.tablePrefix}_queue (
          name,
          dead_letter_queue_name,
          delay_ms,
          message_retention_period_ms,
          visibility_timeout_ms,
          receive_message_wait_time_ms,
          expires_ms,
          expires_at,
          max_receive_count,
          max_message_size,
          tags,
          created_at,
          last_modified_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `
    );

    this.addQuery(
      SQL_FIND_TOPIC_INFOS,
      `
        SELECT
          t.name,
          t.tags,
          s.id as subscription_id,
          s.queue_name as queue_name
        FROM ${this.tablePrefix}_topic t
        LEFT JOIN ${this.tablePrefix}_subscription s ON t.name = s.topic_name
      `
    );

    this.addQuery(
      SQL_FIND_TOPIC_INFO_BY_NAME,
      `
        SELECT
          t.name,
          t.tags,
          s.id as subscription_id,
          s.queue_name as queue_name
        FROM ${this.tablePrefix}_topic t
        LEFT JOIN ${this.tablePrefix}_subscription s ON t.name = s.topic_name
        WHERE
          t.name = ?
      `
    );

    this.addQuery(
      SQL_CREATE_TOPIC,
      `
        INSERT INTO ${this.tablePrefix}_topic (
          name,
          tags,
          created_at,
          last_modified_at
        ) VALUES (?, ?, ?, ?)
      `
    );

    this.addQuery(
      SQL_CREATE_SUBSCRIPTION,
      `
        INSERT INTO ${this.tablePrefix}_subscription (
          id,
          topic_name,
          queue_name
        ) VALUES (?, ?, ?)
      `
    );

    this.addQuery(
      SQL_DELETE_TOPIC,
      `
        DELETE FROM
          ${this.tablePrefix}_topic
        WHERE
          name = ?
      `
    );

    this.addQuery(
      SQL_NAK_MESSAGE,
      `
        UPDATE
          ${this.tablePrefix}_message
        SET
          expires_at = ?,
          last_nak_reason = ?
        WHERE
          queue_name = ?
          AND id = ?
          AND receipt_handle = ?
      `
    );

    this.addQuery(
      SQL_UPDATE_QUEUE_EXPIRES_AT,
      `
        UPDATE
          ${this.tablePrefix}_queue
        SET
          expires_at = ?
        WHERE
          name = ?
      `
    );

    this.addQuery(SQL_DELETE_ALL_MESSAGES, `DELETE FROM ${this.tablePrefix}_message`);
    this.addQuery(SQL_DELETE_ALL_SUBSCRIPTIONS, `DELETE FROM ${this.tablePrefix}_subscription`);
    this.addQuery(SQL_DELETE_ALL_TOPICS, `DELETE FROM ${this.tablePrefix}_topic`);
    this.addQuery(SQL_DELETE_ALL_QUEUES, `DELETE FROM ${this.tablePrefix}_queue`);
    this.addQuery(SQL_DELETE_ALL_USERS, `DELETE FROM ${this.tablePrefix}_user`);
  }

  protected addQuery(queryName: string, sql: string): void {
    this.queries[queryName] = sql;
  }

  public abstract run(db: TDatabase, queryName: string, params: unknown[]): Promise<RunResult>;

  protected abstract runRawSql(db: TDatabase, sql: string, params: unknown[]): Promise<RunResult>;

  public abstract all<TRow>(db: TDatabase, queryName: string, params: unknown[]): Promise<TRow[]>;

  protected getQuery(queryName: string): string {
    const sql = this.queries[queryName];
    if (!sql) {
      throw new Error(`could not find query with name: ${queryName}`);
    }
    return sql;
  }

  public async updateMessage(
    db: TDatabase,
    options: {
      queueName: string;
      messageId: string;
      receiptHandle: string | undefined;
      updateMessageOptions: UpdateMessageOptions;
      now: Date;
    }
  ): Promise<RunResult> {
    const setClauses: unknown[] = [];
    const setParams: unknown[] = [];
    const whereParams: unknown[] = [];
    let sql = `UPDATE ${this.tablePrefix}_message SET`;
    if (options.updateMessageOptions.visibilityTimeoutMs !== undefined) {
      setClauses.push("expires_at=?");
      setParams.push(new Date(options.now.getTime() + options.updateMessageOptions.visibilityTimeoutMs));
    }
    if (options.updateMessageOptions.priority !== undefined) {
      setClauses.push("priority=?");
      setParams.push(options.updateMessageOptions.priority);
    }
    if (options.updateMessageOptions.attributes !== undefined) {
      setClauses.push("attributes=?");
      setParams.push(JSON.stringify(options.updateMessageOptions.attributes));
    }
    if (setParams.length === 0) {
      throw new Error(`expected at least one update but found ${setParams.length}`);
    }
    sql += ` ${setClauses.join(",")}`;

    sql += ` WHERE queue_name=? AND id=?`;
    whereParams.push(options.queueName);
    whereParams.push(options.messageId);

    if (options.receiptHandle !== undefined) {
      sql += ` AND receipt_handle=?`;
      whereParams.push(options.receiptHandle);
    }

    return this.runRawSql(db, sql, [...setParams, ...whereParams]);
  }
}
