import { createLogger } from "@nexq/core";
import sqlite from "better-sqlite3";
import * as R from "radash";
import { Sql } from "./Sql.js";
import { RunResult } from "./dto/RunResult.js";
import { SqlMigration } from "./dto/SqlMigration.js";
import { isTransaction, Transaction } from "../dialect/Transaction.js";
import { isSqliteTransaction } from "../dialect/SqliteTransaction.js";

const logger = createLogger("SqliteSql");
const sqlLogger = createLogger("SQL");

const MIGRATION_VERSION_INITIAL = 1;

export class SqliteSql extends Sql<sqlite.Database> {
  private readonly preparedStatements: Record<string, sqlite.Statement> = {};

  public constructor() {
    super();
  }

  public override async run(
    db: Transaction | sqlite.Database,
    queryName: string,
    params: unknown[]
  ): Promise<RunResult> {
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: run: ${this.getQuery(queryName).trim()}`);
    }
    this.transformParams(params);
    const stmt = this.prepare(db, queryName);
    const result = stmt.run(params);
    return { changes: result.changes };
  }

  protected override async runRawSql(db: sqlite.Database, sql: string, params: unknown[]): Promise<RunResult> {
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: run: ${sql.trim()}`);
    }
    this.transformParams(params);
    const stmt = db.prepare(sql);
    const result = stmt.run(params);
    return { changes: result.changes };
  }

  public override async all<TRow>(
    db: Transaction | sqlite.Database,
    queryName: string,
    params: unknown[]
  ): Promise<TRow[]> {
    if (sqlLogger.isDebugEnabled()) {
      sqlLogger.debug(`sql: all: ${this.getQuery(queryName).trim()}`);
    }
    this.transformParams(params);
    const stmt = this.prepare(db, queryName);
    return stmt.all(params) as TRow[];
  }

  private transformParams(params: unknown[]): void {
    for (let i = 0; i < params.length; i++) {
      const param = params[i];
      if (R.isDate(param)) {
        params[i] = param.toISOString();
      }
    }
  }

  private prepare(db: Transaction | sqlite.Database, queryName: string): sqlite.Statement {
    let stmt = this.preparedStatements[queryName];
    if (stmt) {
      return stmt;
    }
    if (isTransaction(db)) {
      if (isSqliteTransaction(db)) {
        stmt = db.database.prepare(this.getQuery(queryName));
      } else {
        throw new Error("expected SqliteTransaction");
      }
    } else {
      stmt = db.prepare(this.getQuery(queryName));
    }
    this.preparedStatements[queryName] = stmt;
    return stmt;
  }

  public async migrate(database: sqlite.Database): Promise<void> {
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

      database.exec(`
        CREATE TABLE nexq_queue(
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
          dead_letter_topic_name TEXT,
          max_receive_count INTEGER,
          nak_expire_behavior TEXT NOT NULL,
          paused INTEGER NOT NULL,
          tags TEXT NOT NULL,
          FOREIGN KEY(dead_letter_queue_name) REFERENCES nexq_queue(name)
        )
      `);
      database.exec(`CREATE UNIQUE INDEX nexq_queue_name_idx ON nexq_queue(name)`);

      database.exec(`
        CREATE TABLE nexq_topic(
          name TEXT PRIMARY KEY,
          tags TEXT NOT NULL,
          created_at TEXT NOT NULL,
          last_modified_at TEXT NOT NULL
        )
      `);
      database.exec(`CREATE UNIQUE INDEX nexq_topic_name_idx ON nexq_topic(name)`);

      database.exec(`
        CREATE TABLE nexq_subscription(
          id TEXT NOT NULL PRIMARY KEY,
          topic_name TEXT NOT NULL,
          queue_name TEXT NOT NULL,
          UNIQUE (topic_name, queue_name),
          FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE,
          FOREIGN KEY(topic_name) REFERENCES nexq_topic(name) ON DELETE CASCADE
        )
      `);
      database.exec(`CREATE UNIQUE INDEX nexq_subscription_id_idx ON nexq_subscription(id)`);
      database.exec(`CREATE INDEX nexq_subscription_topic_name_idx ON nexq_subscription(topic_name)`);
      database.exec(`CREATE INDEX nexq_subscription_queue_name_idx ON nexq_subscription(queue_name)`);

      database.exec(`
        CREATE TABLE nexq_message(
          id TEXT NOT NULL,
          queue_name TEXT NOT NULL,
          priority INTEGER NOT NULL,
          sent_at TEXT NOT NULL,
          order_by TEXT NOT NULL,
          retain_until TEXT,
          message_body TEXT NOT NULL,
          receive_count INTEGER NOT NULL,
          attributes TEXT NOT NULL,
          expires_at TEXT,
          delay_until TEXT,
          receipt_handle TEXT,
          first_received_at TEXT,
          last_nak_reason TEXT,
          PRIMARY KEY (id, queue_name),
          FOREIGN KEY(queue_name) REFERENCES nexq_queue(name) ON DELETE CASCADE
        )
      `);
      database.exec(`CREATE UNIQUE INDEX nexq_message_id_queue_name_idx ON nexq_message(id, queue_name)`);
      database.exec(`CREATE INDEX nexq_message_queue_name_idx ON nexq_message(queue_name)`);
      database.exec(`CREATE INDEX nexq_message_receipt_handle_idx ON nexq_message(receipt_handle)`);
      database.exec(`CREATE INDEX nexq_message_order_by_idx ON nexq_message(order_by)`);
      database.exec(`CREATE INDEX nexq_message_expires_at_idx ON nexq_message(expires_at)`);
      database.exec(`CREATE INDEX nexq_message_delay_until_idx ON nexq_message(delay_until)`);

      database.exec(`
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
      database.exec(`CREATE UNIQUE INDEX nexq_user_id_idx ON nexq_user(id)`);
      database.exec(`CREATE UNIQUE INDEX nexq_user_username_idx ON nexq_user(username)`);

      database
        .prepare(`INSERT INTO nexq_migration(version, name, applied_at) VALUES (?, ?, ?)`)
        .run(MIGRATION_VERSION_INITIAL, "initial", new Date().toISOString());
    }
  }
}
