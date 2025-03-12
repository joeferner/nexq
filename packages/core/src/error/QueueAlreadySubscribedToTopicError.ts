export class QueueAlreadySubscribedToTopicError extends Error {
  public constructor(
    public readonly topicName: string,
    public readonly queueName: string
  ) {
    super(`queue "${queueName}" already subscribed to topic "${topicName}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
