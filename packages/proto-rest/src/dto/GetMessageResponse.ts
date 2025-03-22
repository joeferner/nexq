export interface GetMessageResponse {
  id: string;
  body: string;
  priority: number;
  attributes: Record<string, string>;

  /**
   * Time the message was originally sent
   */
  sentTime: string;

  positionInQueue: number;
  delayUntil: string | undefined;
  isAvailable: boolean;
  receiveCount: number;
  expiresAt: string | undefined;
  receiptHandle: string | undefined;
  firstReceivedAt: string | undefined;
  lastNakReason: string | undefined;
}
