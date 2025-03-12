import {
  createId,
  createLogger,
  CreateQueueOptions,
  CreateTopicOptions,
  Message,
  QueueInfo,
  ReceiptHandleIsInvalidError,
  SendMessageOptions,
  SendMessageResult,
  Time,
  TopicInfo,
  TopicInfoQueueSubscription,
  TopicProtocol,
  User,
} from "@nexq/core";
import sqlite, { Database } from "better-sqlite3";
import * as R from "radash";
import { SqlStoreCreateConfigSqlite } from "../SqlStore.js";
import { Dialect, DialectCreateUser, Transaction } from "./Dialect.js";

const logger = createLogger("SqliteDialect");

const MIGRATION_VERSION_INITIAL = 1;

export class SqliteDialect implements Dialect {
  private constructor(
    private readonly database: Database,
    private readonly time: Time
  ) {}

  public static async create(options: SqlStoreCreateConfigSqlite, time: Time): Promise<SqliteDialect> {
    const database = sqlite(options.connectionString, options.options);
    await this.migrate(database);
    return new SqliteDialect(database, time);
  }

  public static async migrate(database: Database): Promise<void> {
    database.exec(`CREATE TABLE IF NOT EXISTS nexq_migration(
      version INTEGER,
      name TEXT NOT NULL,
      applied_at TEXT NOT NULL
    )`);

    const migrations = database
      .prepare<[], SqlMigration>(`SELECT version FROM nexq_migration ORDER BY applied_at`)
      .all();

    if (!migrations.some((m) => m.version === MIGRATION_VERSION_INITIAL)) {
      logger.info(`running migration ${MIGRATION_VERSION_INITIAL} - initial`);

      database.exec(`CREATE TABLE nexq_queue(
        name TEXT PRIMARY KEY,
        created_at TEXT NOT NULL,
        last_modified_at TEXT NOT NULL,
        expires_at TEXT,
        expires_ms INTEGER,
        delay_ms INTEGER,
        max_message_size INTEGER,
        message_retention_period_ms INTEGER,
        receive_message_wait_time_ms INTEGER,
        visibility_timeout_ms INTEGER,
        dead_letter_queue_name TEXT,
        max_receive_count INTEGER,
        tags TEXT NOT NULL,
        FOREIGN KEY(dead_letter_queue_name) REFERENCES nexq_queue(name)
      )`);

      database.exec(`CREATE TABLE nexq_topic(
        name TEXT PRIMARY KEY,
        tags TEXT NOT NULL,
        created_at TEXT NOT NULL,
        last_modified_at TEXT NOT NULL
      )`);

      database.exec(`CREATE TABLE nexq_subscription(
        id TEXT NOT NULL PRIMARY KEY,
        topic_name TEXT NOT NULL,
        queue_name TEXT NOT NULL,
        UNIQUE (topic_name, queue_name),
        FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE,
        FOREIGN KEY(topic_name) REFERENCES nexq_topic(name) ON DELETE CASCADE
      )`);

      database.exec(`CREATE TABLE nexq_message(
        id TEXT NOT NULL,
        queue_name TEXT NOT NULL,
        priority INTEGER NOT NULL,
        sent_at TEXT NOT NULL,
        message_body BLOB NOT NULL,
        receive_count INTEGER NOT NULL,
        attributes TEXT NOT NULL,
        expires_at TEXT,
        delay_until TEXT,
        receipt_handle TEXT,
        first_received_at TEXT,
        PRIMARY KEY (id, queue_name),
        FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE
      )`);

      database.exec(`CREATE TABLE nexq_user(
        id TEXT NOT NULL PRIMARY KEY,
        username TEXT NOT NULL,
        password_hash TEXT,
        access_key_id TEXT,
        secret_access_key TEXT,
        UNIQUE (username),
        UNIQUE (access_key_id)
      )`);

      database
        .prepare(`INSERT INTO nexq_migration(version, name, applied_at) VALUES (?, ?, ?)`)
        .run(MIGRATION_VERSION_INITIAL, "initial", new Date().toISOString());
    }
  }

  public async shutdown(): Promise<void> {
    this.database.close();
  }

  public async createTransaction(): Promise<Transaction> {
    return {
      commit: async (): Promise<void> => {},
      rollback: async (): Promise<void> => {},
    };
  }

  public async findUserByAccessKeyId(_tx: Transaction | undefined, accessKeyId: string): Promise<User | undefined> {
    const rows = this.database
      .prepare<[string], SqlUser>(`SELECT * FROM nexq_user WHERE access_key_id = ?`)
      .all(accessKeyId);
    return expect0or1Row(rows, sqlUserToUser);
  }

  public async findUserByUsername(_tx: Transaction | undefined, username: string): Promise<User | undefined> {
    const rows = this.database.prepare<[string], SqlUser>(`SELECT * FROM nexq_user WHERE username = ?`).all(username);
    return expect0or1Row(rows, sqlUserToUser);
  }

  public async createUser(_tx: Transaction, options: DialectCreateUser): Promise<void> {
    this.database
      .prepare(
        `INSERT INTO nexq_user (
        id,
        username,
        password_hash,
        access_key_id,
        secret_access_key
      ) VALUES (?, ?, ?, ?, ?)`
      )
      .run(options.id, options.username, options.passwordHash, options.accessKeyId, options.secretAccessKey);
  }

  public async findUsers(): Promise<User[]> {
    const rows = this.database.prepare<[], SqlUser>(`SELECT * FROM nexq_user`).all();
    return rows.map(sqlUserToUser);
  }

  public async sendMessage(
    queueInfo: QueueInfo,
    body: Buffer,
    options?: SendMessageOptions
  ): Promise<SendMessageResult> {
    const now = this.time.getCurrentTime();
    const id = createId();
    this.database
      .prepare(
        `
      INSERT INTO nexq_message (
        id,
        queue_name,
        priority,
        sent_at,
        message_body,
        receive_count,
        attributes,
        expires_at,
        delay_until
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`
      )
      .run(
        id,
        queueInfo.name,
        options?.priority ?? 0,
        now.toISOString(),
        body,
        0,
        options?.attributes ? JSON.stringify(options.attributes) : "{}",
        queueInfo.messageRetentionPeriodMs !== undefined
          ? new Date(now.getTime() + queueInfo.messageRetentionPeriodMs)
          : null,
        options?.delayMs ? new Date(now.getTime() + options.delayMs).toISOString() : null
      );
    return {
      id,
    };
  }

  public async receiveMessages(
    queueName: string,
    options: { visibilityTimeoutMs: number; count: number }
  ): Promise<Message[]> {
    const now = this.time.getCurrentTime();
    const rows = this.database
      .prepare<[string, string, string, number], SqlMessage>(
        `
        SELECT
          *
        FROM
          nexq_message
        WHERE
          queue_name = ?
          AND (expires_at IS NULL OR ? > expires_at)
          AND (delay_until IS NULL OR ? >= delay_until)
        ORDER BY priority DESC, sent_at
        LIMIT ?
      `
      )
      .all(queueName, now.toISOString(), now.toISOString(), options.count);

    return rows.map((row) => {
      const receiptHandle = createId();
      const newExpireTime = new Date(now.getTime() + options.visibilityTimeoutMs);
      const firstReceivedAt = row.first_received_at ?? now.toISOString();
      const receiveCount = row.receive_count + 1;
      this.database
        .prepare(
          `
        UPDATE
          nexq_message
        SET
          expires_at = ?,
          receipt_handle = ?,
          first_received_at = ?,
          receive_count = ?
        WHERE
          id = ?
          AND queue_name = ?
        `
        )
        .run(newExpireTime.toISOString(), receiptHandle, firstReceivedAt, receiveCount, row.id, queueName);
      return sqlMessageToMessage(row, receiptHandle);
    });
  }

  public async updateMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void> {
    const takenUntil = new Date(this.time.getCurrentTime().getTime() + timeMs);
    const results = this.database
      .prepare<[string, string, string], SqlQueue>(
        `
        UPDATE
          nexq_message
        SET
          expires_at = ?
        WHERE
          queue_name = ?
          AND receipt_handle = ?
      `
      )
      .run(takenUntil.toISOString(), queueName, receiptHandle);
    if (results.changes !== 1) {
      throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
    }
  }

  public async deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void> {
    const results = this.database
      .prepare<[string, string], SqlQueue>(
        `
        DELETE FROM 
          nexq_message
        WHERE
          queue_name = ?
          AND receipt_handle = ?
      `
      )
      .run(queueName, receiptHandle);
    if (results.changes !== 1) {
      throw new ReceiptHandleIsInvalidError(queueName, receiptHandle);
    }
  }

  public async deleteExpiredMessages(queueInfo: QueueInfo): Promise<void> {
    if (queueInfo.maxReceiveCount === undefined) {
      return;
    }
    const now = this.time.getCurrentTime();
    this.database
      .prepare<[string, string, number], SqlQueue>(
        `
        DELETE FROM 
          nexq_message
        WHERE
          queue_name = ?
          AND (expires_at IS NOT NULL AND ? > expires_at)
          AND receive_count >= ?
      `
      )
      .run(queueInfo.name, now.toISOString(), queueInfo.maxReceiveCount);
  }

  public async moveExpiredMessagesToDeadLetter(queueInfo: QueueInfo, deadLetterQueueInfo: QueueInfo): Promise<void> {
    if (queueInfo.maxReceiveCount === undefined) {
      throw new Error("queues with a dead letter queue must have a max receive count");
    }
    const now = this.time.getCurrentTime();
    const newExpiresAt =
      deadLetterQueueInfo.expiresMs === undefined
        ? null
        : new Date(now.getTime() + deadLetterQueueInfo.expiresMs).toISOString();
    this.database
      .prepare<[string, string | null, string, string, string, number], SqlQueue>(
        `
        UPDATE 
          nexq_message
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
      )
      .run(
        deadLetterQueueInfo.name,
        newExpiresAt,
        now.toISOString(),
        queueInfo.name,
        now.toISOString(),
        queueInfo.maxReceiveCount
      );
  }

  public async getQueueInfos(_tx: Transaction | undefined): Promise<QueueInfo[]> {
    const rows = this.database
      .prepare<[], SqlQueue>(
        `SELECT
         q.name,
         q.created_at,
         q.last_modified_at,
         q.expires_at,
         q.expires_ms,
         q.delay_ms,
         q.max_message_size,
         q.message_retention_period_ms,
         q.receive_message_wait_time_ms,
         q.visibility_timeout_ms,
         q.dead_letter_queue_name,
         q.max_receive_count,
         q.tags
       FROM nexq_queue q`
      )
      .all();
    const queueInfos = rows.map((row) => {
      return {
        ...sqlQueueToQueueInfo(row),
        numberOfMessages: 0,
        numberOfMessagesDelayed: 0,
        numberOfMessagesNotVisible: 0,
      } satisfies QueueInfo;
    });
    return R.alphabetical(queueInfos, (t) => t.name);
  }

  public async getQueueInfo(_tx: Transaction | undefined, queueName: string): Promise<QueueInfo | undefined> {
    const now = this.time.getCurrentTime();
    const rows = this.database
      .prepare<[string], SqlQueue>(
        `SELECT
         q.name,
         q.created_at,
         q.last_modified_at,
         q.expires_at,
         q.expires_ms,
         q.delay_ms,
         q.max_message_size,
         q.message_retention_period_ms,
         q.receive_message_wait_time_ms,
         q.visibility_timeout_ms,
         q.dead_letter_queue_name,
         q.max_receive_count,
         q.tags
       FROM nexq_queue q
       WHERE q.name = ?
      `
      )
      .all(queueName);
    if (rows.length === 0) {
      return undefined;
    }
    if (rows.length > 1) {
      throw new Error(`expected 1 row but found ${rows.length} while getting queue info for "${queueName}"`);
    }
    return {
      ...sqlQueueToQueueInfo(rows[0]),
      numberOfMessages: await this.getNumberOfMessages(_tx, queueName, now),
      numberOfMessagesDelayed: await this.getNumberOfDelayedMessages(_tx, queueName, now),
      numberOfMessagesNotVisible: await this.getNumberOfNotVisibleMessages(_tx, queueName, now),
    };
  }

  private async getNumberOfMessages(_tx: Transaction | undefined, queueName: string, now: Date): Promise<number> {
    const rows = this.database
      .prepare<[string, string, string], SqlCount>(
        `
        SELECT
          COUNT(*) as count
        FROM
          nexq_message
        WHERE
          queue_name = ?
          AND (expires_at IS NULL OR expires_at < ?)
          AND (delay_until IS NULL OR ? >= delay_until)
      `
      )
      .all(queueName, now.toISOString(), now.toISOString());
    return rows[0].count;
  }

  private async getNumberOfDelayedMessages(
    _tx: Transaction | undefined,
    queueName: string,
    now: Date
  ): Promise<number> {
    const rows = this.database
      .prepare<[string, string], SqlCount>(
        `
        SELECT
          COUNT(*) as count
        FROM
          nexq_message
        WHERE
          queue_name = ?
          AND delay_until IS NOT NULL
          AND ? < delay_until
      `
      )
      .all(queueName, now.toISOString());
    return rows[0].count;
  }

  private async getNumberOfNotVisibleMessages(
    _tx: Transaction | undefined,
    queueName: string,
    now: Date
  ): Promise<number> {
    const rows = this.database
      .prepare<[string, string], SqlCount>(
        `
        SELECT
          COUNT(*) as count
        FROM
          nexq_message
        WHERE
          queue_name = ?
          AND expires_at IS NOT NULL
          AND expires_at >= ?
      `
      )
      .all(queueName, now.toISOString());
    return rows[0].count;
  }

  public async createQueue(_tx: Transaction, queueName: string, options?: CreateQueueOptions): Promise<void> {
    const now = this.time.getCurrentTime();
    this.database
      .prepare(
        `INSERT INTO nexq_queue (
          name,
          dead_letter_queue_name,
          delay_ms,
          message_retention_period_ms,
          visibility_timeout_ms,
          receive_message_wait_time_ms,
          expires_ms,
          max_receive_count,
          max_message_size,
          tags,
          created_at,
          last_modified_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
      )
      .run(
        queueName,
        options?.deadLetterQueueName ?? null,
        options?.delayMs ?? null,
        options?.messageRetentionPeriodMs ?? null,
        options?.visibilityTimeoutMs ?? null,
        options?.receiveMessageWaitTimeMs ?? null,
        options?.expiresMs ?? null,
        options?.maxReceiveCount ?? null,
        options?.maxMessageSize ?? null,
        options?.tags ? JSON.stringify(options.tags) : "{}",
        now.toISOString(),
        now.toISOString()
      );
  }

  public async getTopicInfos(_tx: Transaction | undefined): Promise<TopicInfo[]> {
    const rows = this.database
      .prepare<[], SqlTopicWithSubscription>(
        `SELECT
         t.name,
         t.tags,
         s.id as subscription_id,
         s.queue_name as queue_name
       FROM nexq_topic t
       LEFT JOIN nexq_subscription s ON t.name = s.topic_name
      `
      )
      .all();
    const topicRows = Object.values(R.group(rows, (r) => r.name)).filter((r) => !!r);
    const topics = R.alphabetical(topicRows, (t) => t[0].name);
    return topics.map(sqlTopicToTopicInfo);
  }

  public async getTopicInfo(_tx: Transaction | undefined, topicName: string): Promise<TopicInfo | undefined> {
    const rows = this.database
      .prepare<[string], SqlTopicWithSubscription>(
        `SELECT
         t.name,
         t.tags,
         s.id as subscription_id,
         s.queue_name as queue_name
       FROM nexq_topic t
       LEFT JOIN nexq_subscription s ON t.name = s.topic_name
       WHERE t.name = ?
      `
      )
      .all(topicName);
    if (rows.length === 0) {
      return undefined;
    }
    return sqlTopicToTopicInfo(rows);
  }

  public async createTopic(_tx: Transaction, topicName: string, options?: CreateTopicOptions): Promise<void> {
    const now = this.time.getCurrentTime();
    this.database
      .prepare(
        `INSERT INTO nexq_topic (
        name,
        tags,
        created_at,
        last_modified_at
      ) VALUES (?, ?, ?, ?)`
      )
      .run(topicName, options?.tags ? JSON.stringify(options.tags) : "{}", now.toISOString(), now.toISOString());
  }

  public async subscribe(_tx: Transaction, _id: string, _topicName: string, _queueName: string): Promise<void> {
    throw new Error("Method not implemented.");
  }
}

function sqlUserToUser(row: SqlUser): User {
  return {
    id: row.id,
    username: row.username,
    passwordHash: row.password_hash ?? undefined,
    accessKeyId: row.access_key_id ?? undefined,
    secretAccessKey: row.secret_access_key ?? undefined,
  };
}

function sqlQueueToQueueInfo(
  row: SqlQueue
): Omit<QueueInfo, "numberOfMessages" | "numberOfMessagesDelayed" | "numberOfMessagesNotVisible"> {
  return {
    name: row.name,
    created: parseDate(row.created_at),
    lastModified: parseDate(row.last_modified_at),
    delayMs: row.delay_ms ?? undefined,
    expiresMs: row.expires_ms ?? undefined,
    expiresAt: parseOptionalDate(row.expires_at),
    maxMessageSize: row.max_message_size ?? undefined,
    messageRetentionPeriodMs: row.message_retention_period_ms ?? undefined,
    receiveMessageWaitTimeMs: row.receive_message_wait_time_ms ?? undefined,
    visibilityTimeoutMs: row.visibility_timeout_ms ?? undefined,
    tags: JSON.parse(row.tags) as Record<string, string>,
    deadLetterQueueName: row.dead_letter_queue_name ?? undefined,
    maxReceiveCount: row.max_receive_count ?? undefined,
  };
}

function sqlTopicToTopicInfo(rows: SqlTopicWithSubscription[]): TopicInfo {
  return {
    name: rows[0].name,
    tags: JSON.parse(rows[0].tags) as Record<string, string>,
    subscriptions: rows.map((row) => {
      return {
        id: row.subscription_id,
        protocol: TopicProtocol.Queue,
        queueName: row.queue_name,
      } satisfies TopicInfoQueueSubscription;
    }),
  };
}

function sqlMessageToMessage(row: SqlMessage, receiptHandle: string): Message {
  return new Message({
    id: row.id,
    priority: row.priority,
    receiptHandle,
    sentTime: new Date(row.sent_at),
    attributes: JSON.parse(row.attributes) as Record<string, string>,
    body: row.message_body,
  });
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

interface SqlMigration {
  version: number;
  name: string;
  applied_at: string;
}

interface SqlUser {
  id: string;
  username: string;
  password_hash: string | null;
  access_key_id: string | null;
  secret_access_key: string | null;
}

interface SqlQueue {
  name: string;
  created_at: string;
  last_modified_at: string;
  expires_at: string | null;
  expires_ms: number | null;
  delay_ms: number | null;
  max_message_size: number | null;
  message_retention_period_ms: number | null;
  receive_message_wait_time_ms: number | null;
  visibility_timeout_ms: number | null;
  dead_letter_queue_name: string | null;
  max_receive_count: number | null;
  tags: string;
}

interface SqlMessage {
  id: string;
  queue_name: string;
  priority: number;
  sent_at: string;
  message_body: Buffer;
  receive_count: number;
  attributes: string;
  expires_at: string | null;
  delay_until: string | null;
  receipt_handle: string | null;
  first_received_at: string | null;
}

interface SqlTopic {
  name: string;
  tags: string;
}

interface SqlCount {
  count: number;
}

interface SqlTopicWithSubscription extends SqlTopic {
  subscription_id: string;
  queue_name: string;
}

function parseDate(dateString: string): Date {
  return new Date(dateString);
}

function parseOptionalDate(dateString: string | null): Date | undefined {
  if (dateString === null) {
    return undefined;
  }
  return parseDate(dateString);
}
