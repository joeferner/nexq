export interface Message {
  id: string;
  body: string;
  /**
   * Time the message was originally sent
   */
  sentTime: Date;
  priority: number;
  lastNakReason: string | undefined;
  attributes: Record<string, string>;
  delayUntil: Date | undefined;
  isAvailable: boolean;
  receiveCount: number;
  expiresAt: Date | undefined;
  receiptHandle: string | undefined;
  firstReceivedAt: Date | undefined;
}

export interface ReceivedMessage extends Message {
  receiptHandle: string;
}

export interface GetMessage extends Message {
  positionInQueue: number;
}

export function isAvailable(
  message: { expiresAt: Date | undefined; delayUntil: Date | undefined },
  now: Date
): boolean {
  if (message.expiresAt !== undefined) {
    if (message.expiresAt >= now) {
      return false;
    }
  }
  return !isDelayed(message, now);
}

export function isDelayed(message: { delayUntil: Date | undefined }, now: Date): boolean {
  if (message.delayUntil !== undefined) {
    if (now < message.delayUntil) {
      return true;
    }
  }
  return false;
}
