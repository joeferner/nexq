import { createLogger, DuplicateMessageError, getErrorMessage, QueueInfo, SendMessageOptions, Time } from "@nexq/core";
import fs from "node:fs";
import pg from "pg";
import Pool from "pg-pool";
import * as R from "radash";
import { SqlStoreCreateConfigPostgres } from "../SqlStore.js";
import { PostgresSql } from "../sql/PostgresSql.js";
import {
  Dialect,
  DialectMessageNotification,
  DialectQueueNotification,
  DialectSubscriptionNotification,
  DialectTopicNotification,
} from "./Dialect.js";
import { PostgresTransaction } from "./PostgresTransaction.js";
import { Transaction } from "./Transaction.js";

interface MessageNotification {
  type: "message-delete" | "message-upsert";
  op: "INSERT" | "UPDATE" | "DELETE";
  id: string;
  queueName: string;
}

interface QueueNotification {
  type: "queue-delete" | "queue-upsert";
  op: "INSERT" | "UPDATE" | "DELETE";
  queueName: string;
}

interface TopicNotification {
  type: "topic-delete" | "topic-upsert";
  op: "INSERT" | "UPDATE" | "DELETE";
  topicName: string;
}

interface SubscriptionNotification {
  type: "subscription-delete" | "subscription-upsert";
  op: "INSERT" | "UPDATE" | "DELETE";
  id: string;
  topicName: string;
  queueName: string;
}

type Notification = MessageNotification | QueueNotification | TopicNotification | SubscriptionNotification;

const logger = createLogger("PostgresDialect");

export class PostgresDialect extends Dialect<Pool<pg.Client>, PostgresSql> {
  private startListenSleep = 0;
  private listenClient?: pg.Client & pg.PoolClient;
  private shuttingDown = false;

  private constructor(sql: PostgresSql, pool: Pool<pg.Client>, time: Time) {
    super(sql, pool, time);
  }

  private async startListen(): Promise<void> {
    if (this.listenClient) {
      throw new Error("already started listening");
    }

    await R.sleep(this.startListenSleep);
    this.listenClient = await this.database.connect();
    this.listenClient.on("end", () => {
      if (this.shuttingDown) {
        return;
      }
      this.startListenSleep = 1 + Math.min(10 * 1000, this.startListenSleep * 2);
      void this.startListen();
    });
    await this.listenClient.query("LISTEN nexq_message_notify");
    await this.listenClient.query("LISTEN nexq_queue_notify");
    await this.listenClient.query("LISTEN nexq_topic_notify");
    await this.listenClient.query("LISTEN nexq_subscription_notify");
    this.listenClient.on("notification", (message) => {
      const payload = message.payload;
      if (!payload) {
        return;
      }

      const payloadObj = JSON.parse(payload) as Notification;
      if (payloadObj.type === "message-delete" || payloadObj.type === "message-upsert") {
        this.emit("messageNotification", {
          type: "dialectMessageNotification",
          op: payloadObj.op,
          id: payloadObj.id,
          queueName: payloadObj.queueName,
        } satisfies DialectMessageNotification);
      } else if (payloadObj.type === "queue-delete" || payloadObj.type === "queue-upsert") {
        this.emit("queueNotification", {
          type: "dialectQueueNotification",
          op: payloadObj.op,
          queueName: payloadObj.queueName,
        } satisfies DialectQueueNotification);
      } else if (payloadObj.type === "topic-delete" || payloadObj.type === "topic-upsert") {
        this.emit("topicNotification", {
          type: "dialectTopicNotification",
          op: payloadObj.op,
          topicName: payloadObj.topicName,
        } satisfies DialectTopicNotification);
      } else if (payloadObj.type === "subscription-delete" || payloadObj.type === "subscription-upsert") {
        this.emit("subscriptionNotification", {
          type: "dialectSubscriptionNotification",
          op: payloadObj.op,
          id: payloadObj.id,
          topicName: payloadObj.topicName,
          queueName: payloadObj.queueName,
        } satisfies DialectSubscriptionNotification);
      } else {
        logger.warn(`unhandled notification type: "${payloadObj.type}"`);
      }
    });
    this.startListenSleep = 0;
  }

  public async beginTransaction(): Promise<PostgresTransaction> {
    const client = await this.database.connect();
    await client.query("BEGIN");

    return {
      client,

      commit: async (): Promise<void> => {
        await client.query("COMMIT");
      },

      rollback: async (): Promise<void> => {
        await client.query("ROLLBACK");
      },

      release: async (): Promise<void> => {
        client.release();
      },
    };
  }

  public static async create(options: SqlStoreCreateConfigPostgres, time: Time): Promise<PostgresDialect> {
    const params = new URL(options.connectionString);
    const database = params.pathname?.split("/")?.[1];
    const port = parseInt(params.port ?? "5432");

    if (options.options?.ssl?.ca) {
      options.options.ssl.ca = await fs.promises.readFile(options.options.ssl.ca, "utf8");
    }
    if (options.options?.ssl?.cert) {
      options.options.ssl.cert = await fs.promises.readFile(options.options.ssl.cert, "utf8");
    }
    if (options.options?.ssl?.key) {
      options.options.ssl.key = await fs.promises.readFile(options.options.ssl.key, "utf8");
    }

    pg.types.setTypeParser(1700, parseFloat);
    pg.types.setTypeParser(20, parseFloat);

    const pool = new Pool({
      user: params.username,
      password: params.password,
      host: params.hostname,
      port,
      database,
      ...options.options,
    });
    const sql = new PostgresSql();
    await sql.migrate(pool);
    const dialect = new PostgresDialect(sql, pool, time);
    await dialect.startListen();
    return dialect;
  }

  public async shutdown(): Promise<void> {
    this.shuttingDown = true;
    if (this.listenClient) {
      this.listenClient.release();
      this.listenClient = undefined;
    }
    await this.database.end();
  }

  protected toSqlBoolean(v: boolean): unknown {
    return v;
  }

  public override async sendMessage(
    tx: Transaction | undefined,
    queueInfo: QueueInfo,
    id: string,
    body: string,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): Promise<void> {
    if (options) {
      options.deduplicationId = options?.deduplicationId ?? id;
    }
    try {
      return await super.sendMessage(tx, queueInfo, id, body, options);
    } catch (err) {
      if (
        options?.deduplicationId &&
        getErrorMessage(err).includes(
          `error: duplicate key value violates unique constraint "nexq_message_deduplication_id"`
        )
      ) {
        throw new DuplicateMessageError(options.deduplicationId);
      }
      throw err;
    }
  }
}
