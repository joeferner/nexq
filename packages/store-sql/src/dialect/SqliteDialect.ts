import { Time } from "@nexq/core";
import sqlite, { Database } from "better-sqlite3";
import { SqlStoreCreateConfigSqlite } from "../SqlStore.js";
import { SqliteSql } from "../sql/SqliteSql.js";
import { Dialect } from "./Dialect.js";

export class SqliteDialect extends Dialect<sqlite.Database> {
  private constructor(sql: SqliteSql, database: Database, time: Time) {
    super(sql, database, time);
  }

  public static async create(options: SqlStoreCreateConfigSqlite, time: Time): Promise<SqliteDialect> {
    const database = sqlite(options.connectionString, options.options);
    const sql = new SqliteSql();
    await sql.migrate(database);
    return new SqliteDialect(sql, database, time);
  }

  public async shutdown(): Promise<void> {
    this.database.close();
  }
}
