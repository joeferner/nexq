import { DuplicateMessageError, getErrorMessage, QueueInfo, SendMessageOptions, Time } from "@nexq/core";
import { Mutex, MutexInterface } from "async-mutex";
import sqlite, { Database } from "better-sqlite3";
import { SqlStoreCreateConfigSqlite } from "../SqlStore.js";
import { SqliteSql } from "../sql/SqliteSql.js";
import { Dialect } from "./Dialect.js";
import { SqliteTransaction } from "./SqliteTransaction.js";
import { Transaction } from "./Transaction.js";

export class SqliteDialect extends Dialect<sqlite.Database, SqliteSql> {
  private txMutex = new Mutex();

  private constructor(sql: SqliteSql, database: Database, time: Time) {
    super(sql, database, time);
  }

  public async beginTransaction(): Promise<SqliteTransaction> {
    let unlock: MutexInterface.Releaser | undefined = await this.txMutex.acquire();
    let commitOrRollbackCompleted = false;
    this.database.exec("BEGIN TRANSACTION");
    return {
      database: this.database,

      commit: async (): Promise<void> => {
        try {
          this.database.exec("COMMIT");
          commitOrRollbackCompleted = true;
        } finally {
          unlock?.();
          unlock = undefined;
        }
      },

      rollback: async (): Promise<void> => {
        try {
          this.database.exec("ROLLBACK");
          commitOrRollbackCompleted = true;
        } finally {
          unlock?.();
          unlock = undefined;
        }
      },

      release: async (): Promise<void> => {
        try {
          if (!commitOrRollbackCompleted) {
            this.database.exec("ROLLBACK");
          }
        } finally {
          unlock?.();
          unlock = undefined;
        }
      },
    };
  }

  public static async create(options: SqlStoreCreateConfigSqlite, time: Time): Promise<SqliteDialect> {
    const database = sqlite(options.connectionString, options.options);
    database.pragma("journal_mode = WAL");
    const sql = new SqliteSql();
    await sql.migrate(database);
    return new SqliteDialect(sql, database, time);
  }

  public async shutdown(): Promise<void> {
    this.database.close();
  }

  protected toSqlBoolean(v: boolean): unknown {
    return v ? 1 : 0;
  }

  public override async sendMessage(
    tx: Transaction | undefined,
    queueInfo: QueueInfo,
    id: string,
    body: string,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): Promise<void> {
    if (options) {
      options.deduplicationId = options.deduplicationId ?? id;
    }
    try {
      return await super.sendMessage(tx, queueInfo, id, body, options);
    } catch (err) {
      if (
        options?.deduplicationId &&
        getErrorMessage(err).includes(`UNIQUE constraint failed: index 'nexq_message_deduplication_id'`)
      ) {
        throw new DuplicateMessageError(options.deduplicationId);
      }
      throw err;
    }
  }
}
