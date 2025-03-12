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
}
