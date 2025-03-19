import { Database } from "better-sqlite3";
import { isTransaction, Transaction } from "./Transaction.js";

export interface SqliteTransaction extends Transaction {
  database: Database;
}

export function isSqliteTransaction(obj: object): obj is SqliteTransaction {
  return isTransaction(obj) && "database" in obj;
}
