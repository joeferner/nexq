export class QueueNotFoundError extends Error {
  public constructor(public readonly queueName: string) {
    super(`queue "${queueName}" not found`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
