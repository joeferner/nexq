export interface GetQueuesResponse {
  queues: GetQueuesResponseQueue[];
}

export interface GetQueuesResponseQueue {
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
  maxReceiveCount?: number;
}
