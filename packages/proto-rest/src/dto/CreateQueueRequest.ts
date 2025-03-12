export interface CreateQueueRequest {
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
   * dead letter queue
   */
  deadLetter?: {
    /**
     * name of the dead letter queue
     *
     * @example "my-queue-dlq"
     */
    queueName: string;

    /**
     * max number of time to receive a message before moving it to the dlq
     *
     * @example "10"
     */
    maxReceiveCount?: number;
  };

  /**
   * tags to apply to this queue
   */
  tags?: Record<string, string>;
}
