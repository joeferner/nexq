import { GetMessage, isAvailable, Message, ReceivedMessage } from "@nexq/core";
import { parseDate, parseOptionalDate } from "../../utils.js";

export interface SqlMessage {
  id: string;
  queue_name: string;
  priority: number;
  sent_at: string | Date;
  order_by: string | Date;
  message_body: string;
  receive_count: number;
  attributes: string;
  expires_at: string | Date | null;
  delay_until: string | Date | null;
  receipt_handle: string | null;
  first_received_at: string | Date | null;
  last_nak_reason: string | null;
}

export function sqlMessageToMessage(row: SqlMessage): Message {
  return {
    id: row.id,
    priority: row.priority,
    sentTime: parseDate(row.sent_at),
    attributes: JSON.parse(row.attributes) as Record<string, string>,
    body: row.message_body,
    lastNakReason: row.last_nak_reason ?? undefined,
  };
}

export function sqlMessageToReceivedMessage(row: SqlMessage, receiptHandle: string): ReceivedMessage {
  return {
    ...sqlMessageToMessage(row),
    receiptHandle,
  };
}

export function sqlMessageToGetMessage(row: SqlMessage, positionInQueue: number, now: Date): GetMessage {
  const result: GetMessage = {
    ...sqlMessageToMessage(row),
    delayUntil: parseOptionalDate(row.delay_until),
    isAvailable: false,
    positionInQueue,
    receiveCount: row.receive_count,
    expiresAt: parseOptionalDate(row.expires_at),
    receiptHandle: row.receipt_handle ?? undefined,
    firstReceivedAt: parseOptionalDate(row.first_received_at),
    lastNakReason: row.last_nak_reason ?? undefined,
  };
  result.isAvailable = isAvailable(result, now);
  return result;
}
