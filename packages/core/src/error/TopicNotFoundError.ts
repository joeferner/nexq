export class TopicNotFoundError extends Error {
  public constructor(public readonly queueName: string) {
    super(`topic "${queueName}" not found`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
