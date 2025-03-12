export interface DialectCreateUser {
  id: string;
  username: string;
  passwordHash: string | null;
  accessKeyId: string | null;
  secretAccessKey: string | null;
}
