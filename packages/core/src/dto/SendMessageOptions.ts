export interface SendMessageOptions {
  attributes?: Record<string, string>;

  /**
   * If set delays the message from being received by this amount
   */
  delayMs?: number;

  /**
   * if specified, higher priorities have higher priority (default: 0)
   */
  priority?: number;

  /**
   * If this ID is present, futures messages with this ID that match this ID
   * will not be allowed. One a message is received, even if it errors or times
   * out will not be considered a duplicate.
   */
  deduplicationId?: string;
}
