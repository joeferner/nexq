export class InvalidUpdateError extends Error {
  public constructor(message: string) {
    super(message);
    Error.captureStackTrace(this, this.constructor);
  }
}
