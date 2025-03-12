export class DeleteDeadLetterQueueError extends Error {
  public constructor(
    public readonly associatedQueueName: string,
    public readonly deadLetterQueueName: string
  ) {
    super(`cannot delete dead letter queue "${deadLetterQueueName}", associated with queue "${associatedQueueName}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
