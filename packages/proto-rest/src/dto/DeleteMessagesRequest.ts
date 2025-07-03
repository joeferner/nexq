export interface DeleteMessagesRequest {
  messages: DeleteMessagesRequestMessage[];
}

export interface DeleteMessagesRequestMessage {
  messageId: string;
  receiptHandle?: string;
}
