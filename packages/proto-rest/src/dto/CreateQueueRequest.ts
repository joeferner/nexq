import { NakExpireBehaviorOptions } from "@nexq/core";

export interface CreateQueueRequest {
  /**
   * If true the queue will either be updated or created (default: false)
   */
  upsert?: boolean;

  /**
   * The name of the queue
   *
   * @example "my-queue"
   */
  name: string;

  /**
   * the default delay on the queue in seconds
   *
   * @example "10ms"
   */
  delay?: string;

  /**
   * The length of time, for which this queue expires, if the queue expires it and all it's messages are deleted
   *
   * @example "1d"
   */
  expires?: string;

  /**
   * the limit of how many bytes a message can contain
   *
   * @example "10mb"
   */
  maximumMessageSize?: string | number;

  /**
   * the length of time, retains a message
   *
   * @example "1d"
   */
  messageRetentionPeriod?: string;

  /**
   * the length of time, for which the ReceiveMessage action waits for a message to arrive
   *
   * @example "10s"
   */
  receiveMessageWaitTime?: string;

  /**
   * The visibility timeout for the queue. If a message visibility timeout has not been extended in this
   * period other clients will be allowed to read the message
   *
   * @example "1m"
   */
  visibilityTimeout?: string;

  /**
   * name of the dead letter queue
   *
   * @example "my-queue-dlq"
   */
  deadLetterQueueName?: string;

  /**
   * name of the dead letter topic
   *
   * @example "my-topic-dlq"
   */
  deadLetterTopicName?: string;

  /**
   * max number of time to receive a message before moving it to the dlq
   *
   * @example "10"
   * @isInt
   */
  maxReceiveCount?: number;

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
   * tags to apply to this queue
   */
  tags?: Record<string, string>;
}
