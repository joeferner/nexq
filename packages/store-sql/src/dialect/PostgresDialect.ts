import { Time } from "@nexq/core";
import pg from "pg";
import Pool from "pg-pool";
import { SqlStoreCreateConfigPostgres } from "../SqlStore.js";
import { PostgresSql } from "../sql/PostgresSql.js";
import { Dialect } from "./Dialect.js";
import { PostgresTransaction } from "./PostgresTransaction.js";

export class PostgresDialect extends Dialect<Pool<pg.Client>, PostgresSql> {
  private constructor(sql: PostgresSql, pool: Pool<pg.Client>, time: Time) {
    super(sql, pool, time);
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
    return new PostgresDialect(sql, pool, time);
  }

  public async shutdown(): Promise<void> {
    await this.database.end();
  }

  protected toSqlBoolean(v: boolean): unknown {
    return v;
  }
}
