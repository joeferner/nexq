export interface PeekMessagesResponse {
  messages: PeekMessagesResponseMessage[];
}

export interface PeekMessagesResponseMessage {
  id: string;
  body: string;
  /**
   * @isInt
   */
  priority: number;
  attributes: Record<string, string>;

  /**
   * Time the message was originally sent
   */
  sentTime: string;
  lastNakReason?: string;
  delayUntil?: string;
  isAvailable: boolean;
  receiveCount: number;
  expiresAt?: string;
  receiptHandle?: string;
  firstReceivedAt?: string;
}
