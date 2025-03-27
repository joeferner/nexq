export class InvalidArgumentError extends Error {
  public constructor(message: string) {
    super(message);
    Error.captureStackTrace(this, this.constructor);
  }
}
