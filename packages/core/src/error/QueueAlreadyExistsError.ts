export class QueueAlreadyExistsError extends Error {
  public constructor(
    public readonly queueName: string,
    public readonly reason: string
  ) {
    super(`queue "${queueName}" already exists: ${reason}`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
