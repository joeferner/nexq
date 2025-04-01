export interface GetMessageResponse {
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

  /**
   * @isInt
   */
  positionInQueue: number;
  delayUntil: string | undefined;
  isAvailable: boolean;
  /**
   * @isInt
   */
  receiveCount: number;
  expiresAt: string | undefined;
  receiptHandle: string | undefined;
  firstReceivedAt: string | undefined;
  lastNakReason: string | undefined;
}
