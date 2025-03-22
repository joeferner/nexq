import { Message, ReceivedMessage } from "@nexq/core";
import { parseDate } from "../../utils.js";

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
