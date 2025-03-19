import { isTransaction, Transaction } from "./Transaction.js";
import { Client as PgClient } from "pg";
import * as pg from "pg";

export interface PostgresTransaction extends Transaction {
  client: PgClient & pg.PoolClient;
}

export function isPostgresTransaction(obj: object): obj is PostgresTransaction {
  return isTransaction(obj) && "client" in obj;
}
