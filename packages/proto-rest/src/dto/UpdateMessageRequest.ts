export interface UpdateMessageRequest {
  /**
   * New priority for the message
   *
   * @example 5
   */
  priority?: number;

  /**
   * New attributes for the message, this will replace any existing attributes
   */
  attributes?: Record<string, string>;

  /**
   * The visibility timeout for the message. If a message visibility timeout has not been extended in this
   * period other clients will be allowed to read the message.
   *
   * This value can only be set when using a receipt handle.
   *
   * Setting this value to zero is the same as nak'ing the message.
   *
   * @example "1m"
   */
  visibilityTimeout?: string;
}
