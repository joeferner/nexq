export type DecreasePriorityByNakExpireBehavior = { decreasePriorityBy: number };
export type NakExpireBehaviorOptions = "retry" | "moveToEnd" | DecreasePriorityByNakExpireBehavior;

export function isDecreasePriorityByNakExpireBehavior(option: unknown): option is DecreasePriorityByNakExpireBehavior {
  if (!option || !(typeof option === "object")) {
    return false;
  }
  return "decreasePriorityBy" in option;
}

export interface CreateQueueOptions {
  /**
   * If true the queue will either be updated or created (default: false)
   */
  upsert?: boolean;

  /**
   * Optional dead letter queue in which messages that either have reached
   * their max receive count or have been nak'ed will be sent.
   */
  deadLetterQueueName?: string;

  /**
   * Optional dead letter topic in which messages that either have reached
   * their max receive count or have been nak'ed will be sent.
   */
  deadLetterTopicName?: string;

  /**
   * The number of times a message is delivered to the source queue before
   * being moved to the dead-letter queue.
   */
  maxReceiveCount?: number;

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
   * Determines the behavior of messages when they are either nak'ed or have
   * expired visibility timeout. If the message has exceeded it's maxReceiveCount
   * the message will be moved to the dead letter queue irregardless of this
   * setting.
   *
   * retry     - the message is kept at the same position (default)
   * moveToEnd - the message is moved to the end of the queue
   * { decreasePriorityBy: number } - decrease the priority of messages by the given amount
   */
  nakExpireBehavior?: NakExpireBehaviorOptions;

  /**
   * maximum size a message can be
   */
  maxMessageSize?: number;

  /**
   * tags to apply to this queue
   */
  tags?: Record<string, string>;
}
