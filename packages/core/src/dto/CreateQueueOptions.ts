export interface CreateQueueOptions {
  /**
   * Optional dead letter queue in which messages that either have reached
   * their max receive count or have been nak'ed will be sent.
   */
  deadLetterQueueName?: string;

  /**
   * The length of time, for which the delivery of all messages in the queue
   * is delayed.
   */
  delayMs?: number;

  /**
   * The length of time, a messages is retained. Messages older than this
   * duration will be deleted.
   */
  messageRetentionPeriodMs?: number;

  /**
   * The visibility timeout for the queue. If a message visibility timeout
   * has not been extended in this period other clients will be allowed to
   * read the message
   */
  visibilityTimeoutMs?: number;

  /**
   * The length of time, for which a ReceiveMessage action waits for a
   * message to arrive.
   */
  receiveMessageWaitTimeMs?: number;

  /**
   * The length of time, for which this queue expires, if the queue expires
   * it and all it's messages are deleted
   */
  expiresMs?: number;

  /**
   * The number of times a message is delivered to the source queue before
   * being moved to the dead-letter queue.
   */
  maxReceiveCount?: number;

  /**
   * maximum size a message can be
   */
  maxMessageSize?: number;

  /**
   * tags to apply to this queue
   */
  tags?: Record<string, string>;
}
