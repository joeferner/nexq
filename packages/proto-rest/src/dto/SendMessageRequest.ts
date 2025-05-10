export interface SendMessageRequest {
  /**
   * The body of the queue message
   *
   * @example "test message"
   */
  body: string;

  /**
   * delay before the message can be delivered
   *
   * @example "5s"
   */
  delay?: string;

  /**
   * priority to give the message, higher priority messages will be delivered first
   *
   * @example "5"
   * @isInt
   */
  priority?: number;

  /**
   * attributes to include with the message
   */
  attributes?: Record<string, string>;

  /**
   * If this ID is present, futures messages with this ID that match this ID
   * will not be allowed. One a message is received, even if it errors or times
   * out will not be considered a duplicate.
   */
  deduplicationId?: string;
}
