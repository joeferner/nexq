export interface ReceiveMessagesResponse {
  messages: ReceiveMessagesResponseMessage[];
}

export interface ReceiveMessagesResponseMessage {
  id: string;
  receiptHandle: string;
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
}
