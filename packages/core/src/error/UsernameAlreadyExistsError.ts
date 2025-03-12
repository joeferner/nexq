export class UsernameAlreadyExistsError extends Error {
  public constructor(public readonly username: string) {
    super(`user with username "${username}" already exists`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
