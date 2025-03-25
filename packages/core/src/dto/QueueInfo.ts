import * as R from "radash";
import { CreateQueueOptions, NakExpireBehaviorOptions } from "./CreateQueueOptions.js";
import { DEFAULT_NAK_EXPIRE_BEHAVIOR } from "../Store.js";

export interface QueueInfo {
  name: string;
  numberOfMessages: number;
  numberOfMessagesDelayed: number;
  numberOfMessagesNotVisible: number;
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
  nakExpireBehavior: NakExpireBehaviorOptions;
  paused: boolean;
}

export function queueInfoEqualCreateQueueOptions(
  queueInfo: QueueInfo,
  options: CreateQueueOptions
): true | { reason: string } {
  if (queueInfo.deadLetterQueueName !== options.deadLetterQueueName) {
    return { reason: "deadLetterQueueName is different" };
  }
  if (queueInfo.deadLetterTopicName !== options.deadLetterTopicName) {
    return { reason: "deadLetterTopicName is different" };
  }
  if (queueInfo.delayMs !== options.delayMs) {
    return { reason: "delayMs is different" };
  }
  if (queueInfo.messageRetentionPeriodMs !== options.messageRetentionPeriodMs) {
    return { reason: "messageRetentionPeriodMs is different" };
  }
  if (queueInfo.visibilityTimeoutMs !== options.visibilityTimeoutMs) {
    return { reason: "visibilityTimeoutMs is different" };
  }
  if (queueInfo.receiveMessageWaitTimeMs !== options.receiveMessageWaitTimeMs) {
    return { reason: "receiveMessageWaitTimeMs is different" };
  }
  if (queueInfo.expiresMs !== options.expiresMs) {
    return { reason: "expiresMs is different" };
  }
  if (queueInfo.maxReceiveCount !== options.maxReceiveCount) {
    return { reason: "maxReceiveCount is different" };
  }
  if (queueInfo.maxMessageSize !== options.maxMessageSize) {
    return { reason: "maxMessageSize is different" };
  }
  if (!R.isEqual(queueInfo.tags ?? {}, options.tags ?? {})) {
    return { reason: "tags are different" };
  }
  if (
    !R.isEqual(
      queueInfo.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR,
      options.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR
    )
  ) {
    return { reason: "nakExpireBehavior are different" };
  }
  return true;
}
