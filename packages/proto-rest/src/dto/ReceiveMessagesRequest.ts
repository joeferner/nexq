export interface ReceiveMessagesRequest {
  maxNumberOfMessages?: number;
  visibilityTimeout?: string;
  waitTime?: string;
}
