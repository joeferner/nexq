export class InvalidQueueNameError extends Error {
  public constructor(public readonly queueName: string) {
    super(`queue name "${queueName}" is invalid`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
