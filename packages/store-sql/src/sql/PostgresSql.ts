import { createLogger } from "@nexq/core";
import * as pg from "pg";
import { Client as PgClient } from "pg";
import Pool from "pg-pool";
import { isPostgresTransaction } from "../dialect/PostgresTransaction.js";
import { isTransaction, Transaction } from "../dialect/Transaction.js";
import { Sql } from "./Sql.js";
import { RunResult } from "./dto/RunResult.js";
import { SqlMigration } from "./dto/SqlMigration.js";

const logger = createLogger("PostgresSql");
const sqlLogger = createLogger("SQL");

const MIGRATION_VERSION_INITIAL = 1;

export class PostgresSql extends Sql<Pool<PgClient>> {
  public constructor() {
    super();
  }

  protected get supportsForUpdate(): boolean {
    return true;
  }

  protected override getQuery(queryName: string): string {
    const query = super.getQuery(queryName);
    return this.transformSql(query);
  }

  private transformSql(query: string): string {
    let i = 1;
    return query.replaceAll(/\?/g, () => {
      return "$" + i++;
    });
  }

  public override async run(
    db: Transaction | Pool<PgClient>,
    queryName: string,
    params: unknown[]
  ): Promise<RunResult> {
    const sql = this.getQuery(queryName).trim();
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: run: ${sql}`);
    }
    this.transformParams(params);
    const results = await this.getClient(db).query({
      name: queryName,
      text: sql,
      values: params,
    });
    return { changes: results.rowCount ?? 0 };
  }

  protected override async runRawSql(db: Pool<PgClient>, sql: string, params: unknown[]): Promise<RunResult> {
    sql = this.transformSql(sql);
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: run: ${sql.trim()}`);
    }
    this.transformParams(params);
    const results = await db.query(sql, params);
    return { changes: results.rowCount ?? 0 };
  }

  public override async all<TRow>(
    db: Transaction | Pool<PgClient>,
    queryName: string,
    params: unknown[]
  ): Promise<TRow[]> {
    const sql = this.getQuery(queryName).trim();
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: all: ${sql}`);
    }
    this.transformParams(params);
    const results = await this.getClient(db).query({
      name: queryName,
      text: sql,
      values: params,
    });
    return results.rows as TRow[];
  }

  private getClient(db: Pool<PgClient> | Transaction): Pool<PgClient> | (PgClient & pg.PoolClient) {
    if (isTransaction(db)) {
      if (isPostgresTransaction(db)) {
        return db.client;
      } else {
        throw new Error(`expected PostgresTransaction`);
      }
    } else {
      return db;
    }
  }

  private transformParams(_params: unknown[]): void {
    // nothing to transform
  }

  public async migrate(database: Pool<PgClient>): Promise<void> {
    await database.query(`
      CREATE TABLE IF NOT EXISTS nexq_migration(
        version INTEGER,
        name TEXT NOT NULL,
        applied_at TIMESTAMP NOT NULL
      )
    `);

    const migrations = await database.query<SqlMigration>(`SELECT version FROM nexq_migration ORDER BY applied_at`);

    if (!migrations.rows.some((m) => m.version === MIGRATION_VERSION_INITIAL)) {
      logger.info(`running migration ${MIGRATION_VERSION_INITIAL} - initial`);

      await database.query(`
        CREATE TABLE nexq_queue(
          name TEXT PRIMARY KEY,
          created_at TIMESTAMP NOT NULL,
          last_modified_at TIMESTAMP NOT NULL,
          expires_at TIMESTAMP,
          expires_ms INTEGER,
          delay_ms INTEGER,
          max_message_size INTEGER,
          message_retention_period_ms INTEGER,
          receive_message_wait_time_ms INTEGER,
          visibility_timeout_ms INTEGER,
          dead_letter_queue_name TEXT,
          dead_letter_topic_name TEXT,
          max_receive_count INTEGER,
          nak_expire_behavior TEXT NOT NULL,
          paused BOOLEAN NOT NULL,
          tags TEXT NOT NULL,
          FOREIGN KEY(dead_letter_queue_name) REFERENCES nexq_queue(name)
        )
      `);
      await database.query(`CREATE UNIQUE INDEX nexq_queue_name_idx ON nexq_queue(name)`);

      await database.query(`
        CREATE TABLE nexq_topic(
          name TEXT PRIMARY KEY,
          tags TEXT NOT NULL,
          created_at TIMESTAMP NOT NULL,
          last_modified_at TIMESTAMP NOT NULL
        )
      `);
      await database.query(`CREATE UNIQUE INDEX nexq_topic_name_idx ON nexq_topic(name)`);

      await database.query(`
        CREATE TABLE nexq_subscription(
          id TEXT NOT NULL PRIMARY KEY,
          topic_name TEXT NOT NULL,
          queue_name TEXT NOT NULL,
          UNIQUE (topic_name, queue_name),
          FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE,
          FOREIGN KEY(topic_name) REFERENCES nexq_topic(name) ON DELETE CASCADE
        )
      `);
      await database.query(`CREATE UNIQUE INDEX nexq_subscription_id_idx ON nexq_subscription(id)`);
      await database.query(`CREATE INDEX nexq_subscription_topic_name_idx ON nexq_subscription(topic_name)`);
      await database.query(`CREATE INDEX nexq_subscription_queue_name_idx ON nexq_subscription(queue_name)`);

      await database.query(`
        CREATE TABLE nexq_message(
          id TEXT NOT NULL,
          queue_name TEXT NOT NULL,
          priority INTEGER NOT NULL,
          sent_at TIMESTAMP NOT NULL,
          order_by TIMESTAMP NOT NULL,
          retain_until TEXT,
          message_body TEXT NOT NULL,
          receive_count INTEGER NOT NULL,
          attributes TEXT NOT NULL,
          expires_at TIMESTAMP,
          delay_until TIMESTAMP,
          receipt_handle TEXT,
          first_received_at TIMESTAMP,
          last_nak_reason TEXT,
          PRIMARY KEY (id, queue_name),
          FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE
        )
      `);
      await database.query(`CREATE UNIQUE INDEX nexq_message_id_queue_name_idx ON nexq_message(id, queue_name)`);
      await database.query(`CREATE INDEX nexq_message_queue_name_idx ON nexq_message(queue_name)`);
      await database.query(`CREATE INDEX nexq_message_receipt_handle_idx ON nexq_message(receipt_handle)`);
      await database.query(`CREATE INDEX nexq_message_order_by_idx ON nexq_message(order_by)`);
      await database.query(`CREATE INDEX nexq_message_expires_at_idx ON nexq_message(expires_at)`);
      await database.query(`CREATE INDEX nexq_message_delay_until_idx ON nexq_message(delay_until)`);

      await database.query(`
        CREATE TABLE nexq_user(
          id TEXT NOT NULL PRIMARY KEY,
          username TEXT NOT NULL,
          password_hash TEXT,
          access_key_id TEXT,
          secret_access_key TEXT,
          UNIQUE (username),
          UNIQUE (access_key_id)
        )
      `);
      await database.query(`CREATE UNIQUE INDEX nexq_user_id_idx ON nexq_user(id)`);
      await database.query(`CREATE UNIQUE INDEX nexq_user_username_idx ON nexq_user(username)`);

      await database.query(`INSERT INTO nexq_migration(version, name, applied_at) VALUES ($1, $2, $3)`, [
        MIGRATION_VERSION_INITIAL,
        "initial",
        new Date().toISOString(),
      ]);
    }
  }
}
