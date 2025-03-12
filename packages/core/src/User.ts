export interface User {
  id: string;
  username: string;
  passwordHash?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
}
