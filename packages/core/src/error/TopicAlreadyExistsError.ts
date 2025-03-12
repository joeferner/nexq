export class TopicAlreadyExistsError extends Error {
  public constructor(public readonly queueName: string) {
    super(`topic "${queueName}" already exists`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
