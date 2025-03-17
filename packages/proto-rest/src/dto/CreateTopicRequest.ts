export interface CreateTopicRequest {
  /**
   * The name of the topic
   *
   * @example "my-topic"
   */
  name: string;

  /**
   * tags to apply to this topic
   */
  tags?: Record<string, string>;
}
