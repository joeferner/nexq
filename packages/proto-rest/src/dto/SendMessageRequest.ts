export interface SendMessageRequest {
  /**
   * The body of the queue message
   *
   * @example "message body"
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
   */
  priority?: number;

  /**
   * attributes to include with the message
   */
  attributes?: Record<string, string>;
}
