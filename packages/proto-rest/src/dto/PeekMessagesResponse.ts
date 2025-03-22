export interface PeekMessagesResponse {
  messages: PeekMessagesResponseMessage[];
}

export interface PeekMessagesResponseMessage {
  id: string;
  body: string;
  priority: number;
  attributes: Record<string, string>;

  /**
   * Time the message was originally sent
   */
  sentTime: string;
}
