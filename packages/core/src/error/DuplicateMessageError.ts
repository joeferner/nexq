export class DuplicateMessageError extends Error {
  public constructor(public readonly deduplicationId: string) {
    super(`Message with "${deduplicationId}" already exists in queue`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
