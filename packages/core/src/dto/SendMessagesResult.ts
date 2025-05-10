export interface SendMessagesResult {
  results: SendMessagesResultMessage[];
}

export interface SendMessagesResultMessage {
  id?: string;
  error?: string;
}
