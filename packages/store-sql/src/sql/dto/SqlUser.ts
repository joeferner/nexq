import { User } from "@nexq/core";

export interface SqlUser {
  id: string;
  username: string;
  password_hash: string | null;
  access_key_id: string | null;
  secret_access_key: string | null;
}

export function sqlUserToUser(row: SqlUser): User {
  return {
    id: row.id,
    username: row.username,
    passwordHash: row.password_hash ?? undefined,
    accessKeyId: row.access_key_id ?? undefined,
    secretAccessKey: row.secret_access_key ?? undefined,
  };
}
