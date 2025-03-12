export class UserAccessKeyIdAlreadyExistsError extends Error {
  public constructor(public readonly accessKeyId: string) {
    super(`user with access key id "${accessKeyId}" already exists`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
