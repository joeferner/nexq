export interface SendMessagesResponse {
  results: SendMessagesResponseMessage[];
}

export interface SendMessagesResponseMessage {
  id?: string;
  error?: string;
}
