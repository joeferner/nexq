export class ReceiptHandleIsInvalidError extends Error {
  public constructor(
    public readonly queueName: string,
    public readonly receiptHandle: string
  ) {
    super(`receipt handle "${receiptHandle}" is invalid for queue "${queueName}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
