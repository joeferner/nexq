import { QueueInfo } from "@nexq/core";
import { parseDate, parseOptionalDate } from "../../utils.js";

export interface SqlQueue {
  name: string;
  created_at: string | Date;
  last_modified_at: string | Date;
  expires_at: string | Date | null;
  expires_ms: number | null;
  delay_ms: number | null;
  max_message_size: number | null;
  message_retention_period_ms: number | null;
  receive_message_wait_time_ms: number | null;
  visibility_timeout_ms: number | null;
  dead_letter_queue_name: string | null;
  max_receive_count: number | null;
  tags: string;
}

export function sqlQueueToQueueInfo(
  row: SqlQueue
): Omit<QueueInfo, "numberOfMessages" | "numberOfMessagesDelayed" | "numberOfMessagesNotVisible"> {
  return {
    name: row.name,
    created: parseDate(row.created_at),
    lastModified: parseDate(row.last_modified_at),
    delayMs: row.delay_ms ?? undefined,
    expiresMs: row.expires_ms ?? undefined,
    expiresAt: parseOptionalDate(row.expires_at),
    maxMessageSize: row.max_message_size ?? undefined,
    messageRetentionPeriodMs: row.message_retention_period_ms ?? undefined,
    receiveMessageWaitTimeMs: row.receive_message_wait_time_ms ?? undefined,
    visibilityTimeoutMs: row.visibility_timeout_ms ?? undefined,
    tags: JSON.parse(row.tags) as Record<string, string>,
    deadLetterQueueName: row.dead_letter_queue_name ?? undefined,
    maxReceiveCount: row.max_receive_count ?? undefined,
  };
}
