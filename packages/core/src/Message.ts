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
}

export interface ReceivedMessage extends Message {
  receiptHandle: string;
}
