export class InvalidTopicNameError extends Error {
  public constructor(public readonly topicName: string) {
    super(`topic name "${topicName}" is invalid`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
