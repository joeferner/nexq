import { QueueInfo } from "@nexq/core";

export interface GetQueueResponse {
  name: string;
  numberOfMessage: number;
  numberOfMessagesNotVisible: number;
  numberOfMessagesDelayed: number;
  created: Date;
  lastModified: Date;
  delayMs?: number;
  expiresMs?: number;
  expiresAt?: Date;
  maxMessageSize?: number;
  messageRetentionPeriodMs?: number;
  receiveMessageWaitTimeMs?: number;
  visibilityTimeoutMs?: number;
  tags: Record<string, string>;
  deadLetterQueueName?: string;
  deadLetterTopicName?: string;
  maxReceiveCount?: number;
  paused: boolean;
}

export function queueInfoToGetQueueResponse(q: QueueInfo): GetQueueResponse {
  return {
    name: q.name,
    numberOfMessage: q.numberOfMessages,
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
