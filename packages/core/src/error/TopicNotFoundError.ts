export class TopicNotFoundError extends Error {
  public constructor(public readonly topicName: string) {
    super(`topic "${topicName}" not found`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
