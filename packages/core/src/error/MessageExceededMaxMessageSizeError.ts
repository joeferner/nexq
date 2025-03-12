export class MessageExceededMaxMessageSizeError extends Error {
  public constructor(
    public readonly messageSize: number,
    public readonly maxMessageSize: number
  ) {
    super(`message of size ${messageSize} exceeded the maximum message size of ${maxMessageSize}`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
