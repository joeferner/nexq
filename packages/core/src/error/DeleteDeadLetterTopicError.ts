export class DeleteDeadLetterTopicError extends Error {
  public constructor(
    public readonly associatedQueueName: string,
    public readonly deadLetterTopicName: string
  ) {
    super(`cannot delete dead letter topic "${deadLetterTopicName}", associated with queue "${associatedQueueName}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
