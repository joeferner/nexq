import { createId, CreateUserOptions, hashPassword, User } from "@nexq/core";

export class MemoryUser implements User {
  public readonly id: string;
  public readonly username: string;
  public readonly passwordHash?: string;
  public readonly accessKeyId?: string;
  public readonly secretAccessKey?: string;

  public constructor(options: CreateUserOptions, passwordHash: string | undefined) {
    this.id = createId();
    this.username = options.username;
    this.passwordHash = passwordHash;
    this.accessKeyId = options.accessKeyId;
    this.secretAccessKey = options.secretAccessKey;
  }

  public static async createUser(options: CreateUserOptions, passwordHashRounds: number): Promise<MemoryUser> {
    const passwordHash = options.password ? await hashPassword(options.password, passwordHashRounds) : undefined;
    return new MemoryUser(options, passwordHash);
  }
}
