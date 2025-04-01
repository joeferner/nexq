import { QueueInfo } from "@nexq/core";

export interface GetQueueResponse {
  name: string;
  /**
   * @isInt
   */
  numberOfMessages: number;
  /**
   * @isInt
   */
  numberOfMessagesVisible: number;
  /**
   * @isInt
   */
  numberOfMessagesNotVisible: number;
  /**
   * @isInt
   */
  numberOfMessagesDelayed: number;
  created: Date;
  lastModified: Date;
  /**
   * @isInt
   */
  delayMs?: number;
  /**
   * @isInt
   */
  expiresMs?: number;
  expiresAt?: Date;
  /**
   * @isInt
   */
  maxMessageSize?: number;
  /**
   * @isInt
   */
  messageRetentionPeriodMs?: number;
  /**
   * @isInt
   */
  receiveMessageWaitTimeMs?: number;
  /**
   * @isInt
   */
  visibilityTimeoutMs?: number;
  tags: Record<string, string>;
  deadLetterQueueName?: string;
  deadLetterTopicName?: string;
  /**
   * @isInt
   */
  maxReceiveCount?: number;
  paused: boolean;
}

export function queueInfoToGetQueueResponse(q: QueueInfo): GetQueueResponse {
  return {
    name: q.name,
    numberOfMessages: q.numberOfMessages,
    numberOfMessagesVisible: q.numberOfMessagesVisible,
    numberOfMessagesNotVisible: q.numberOfMessagesNotVisible,
    numberOfMessagesDelayed: q.numberOfMessagesDelayed,
    created: q.created,
    lastModified: q.lastModified,
    delayMs: q.delayMs,
    expiresMs: q.expiresMs,
    expiresAt: q.expiresAt,
    maxMessageSize: q.maxMessageSize,
    messageRetentionPeriodMs: q.messageRetentionPeriodMs,
    receiveMessageWaitTimeMs: q.receiveMessageWaitTimeMs,
    visibilityTimeoutMs: q.visibilityTimeoutMs,
    tags: q.tags,
    deadLetterQueueName: q.deadLetterQueueName,
    deadLetterTopicName: q.deadLetterTopicName,
    maxReceiveCount: q.maxReceiveCount,
    paused: q.paused,
  } satisfies GetQueueResponse;
}
