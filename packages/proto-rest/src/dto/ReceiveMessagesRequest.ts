export interface ReceiveMessagesRequest {
  /**
   * @isInt
   */
  maxNumberOfMessages?: number;
  visibilityTimeout?: string;
  waitTime?: string;
}
