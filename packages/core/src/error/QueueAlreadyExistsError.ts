export class QueueAlreadyExistsError extends Error {
  public constructor(public readonly queueName: string) {
    super(`queue "${queueName}" already exists`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
