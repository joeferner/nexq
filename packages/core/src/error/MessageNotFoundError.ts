export class MessageNotFoundError extends Error {
  public constructor(
    public readonly queueName: string,
    public readonly id: string
  ) {
    super(`message id "${id}" is invalid for queue "${queueName}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
