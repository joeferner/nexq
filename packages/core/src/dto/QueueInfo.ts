import * as R from "radash";
import { CreateQueueOptions } from "./CreateQueueOptions.js";

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
  maxReceiveCount?: number;
}

export function queueInfoEqualCreateQueueOptions(queueInfo: QueueInfo, options: CreateQueueOptions): boolean {
  if (queueInfo.deadLetterQueueName !== options.deadLetterQueueName) {
    return false;
  }
  if (queueInfo.delayMs !== options.delayMs) {
    return false;
  }
  if (queueInfo.messageRetentionPeriodMs !== options.messageRetentionPeriodMs) {
    return false;
  }
  if (queueInfo.visibilityTimeoutMs !== options.visibilityTimeoutMs) {
    return false;
  }
  if (queueInfo.receiveMessageWaitTimeMs !== options.receiveMessageWaitTimeMs) {
    return false;
  }
  if (queueInfo.expiresMs !== options.expiresMs) {
    return false;
  }
  if (queueInfo.maxReceiveCount !== options.maxReceiveCount) {
    return false;
  }
  if (queueInfo.maxMessageSize !== options.maxMessageSize) {
    return false;
  }
  if (!R.isEqual(queueInfo.tags, options.tags)) {
    return false;
  }
  return true;
}
