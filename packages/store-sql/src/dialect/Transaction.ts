export interface Transaction {
  commit: () => Promise<void>;
  rollback: () => Promise<void>;
  release: () => Promise<void>;
}

export function isTransaction(obj: object): obj is Transaction {
  return "commit" in obj && "rollback" in obj;
}
