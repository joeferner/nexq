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
}
