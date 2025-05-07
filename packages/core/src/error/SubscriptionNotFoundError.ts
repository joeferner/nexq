export class SubscriptionNotFoundError extends Error {
  public constructor(public readonly subscriptionId: string) {
    super(`subscription "${subscriptionId}" not found`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}
