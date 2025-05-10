import { createId, GetMessage, Message, ReceivedMessage, UpdateMessageOptions } from "@nexq/core";

export interface CreateMemoryQueueMessageOptions {
  id: string;
  priority: number;
  sentTime: Date;
  attributes: Record<string, string>;
  body: string;
  delayUntil: Date | undefined;
  lastNakReason?: string;
  retainUntil: Date | undefined;
  deduplicationId: string | undefined;
}

export class MemoryQueueMessage {
  public readonly id: string;
  public readonly body: string;
  public attributes: Record<string, string>;
  public priority: number;
  /**
   * used with visibility timeout to determine how long the message is taken by the caller
   */
  public expiresAt?: Date;
  /**
   * used with message retention time to determine when the message should be deleted
   */
  public retainUntil?: Date;
  private _receiveCount = 0;
  private _receiptHandle?: string;
  private _firstReceiveTime?: Date;
  private _delayUntil?: Date;
  /**
   * Time the message was originally sent
   */
  public readonly sentTime: Date;
  public orderTime: Date;
  public lastNakReason: string | undefined;
  public readonly deduplicationId: string | undefined;

  public constructor(options: CreateMemoryQueueMessageOptions) {
    this.id = options.id;
    this.body = options.body;
    this.attributes = options.attributes;
    this.sentTime = options.sentTime;
    this.orderTime = options.sentTime;
    this.priority = options.priority;
    this._delayUntil = options.delayUntil;
    this.lastNakReason = options.lastNakReason;
    this.retainUntil = options.retainUntil;
    this.deduplicationId = options.deduplicationId;
  }

  public get receiptHandle(): string | undefined {
    return this._receiptHandle;
  }

  public get receiveCount(): number {
    return this._receiveCount;
  }

  public markReceived(newExpiresTime: Date, now: Date): ReceivedMessage {
    this.expiresAt = newExpiresTime;
    this._receiptHandle = createId();
    if (this._firstReceiveTime === undefined) {
      this._firstReceiveTime = now;
    }
    this._receiveCount++;
    return { ...this.toMessage(now), receiptHandle: this._receiptHandle };
  }

  public toMessage(now: Date): Message {
    return {
      id: this.id,
      body: this.body,
      sentTime: this.sentTime,
      priority: this.priority,
      lastNakReason: this.lastNakReason,
      attributes: this.attributes,
      delayUntil: this._delayUntil,
      isAvailable: this.isAvailable(now),
      receiveCount: this.receiveCount,
      expiresAt: this.expiresAt,
      receiptHandle: this.receiptHandle,
      firstReceivedAt: this._firstReceiveTime,
    };
  }

  public toGetMessage(positionInQueue: number, now: Date): GetMessage {
    return {
      ...this.toMessage(now),
      positionInQueue,
    } satisfies GetMessage;
  }

  public isAvailable(now: Date): boolean {
    if (this.expiresAt !== undefined) {
      if (this.expiresAt >= now) {
        return false;
      }
    }
    return !this.isDelayed(now);
  }

  public isDelayed(now: Date): boolean {
    if (this._delayUntil !== undefined) {
      if (now < this._delayUntil) {
        return true;
      }
    }
    return false;
  }

  public isNotVisible(now: Date): boolean {
    return !this.isAvailable(now) && !this.isDelayed(now);
  }

  public compareTo(other: MemoryQueueMessage): number {
    const priorityDiff = other.priority - this.priority;
    if (priorityDiff !== 0) {
      return priorityDiff;
    }

    return this.orderTime.getTime() - other.orderTime.getTime();
  }

  public update(now: Date, updateMessageOptions: UpdateMessageOptions): void {
    if (updateMessageOptions.priority !== undefined) {
      this.priority = updateMessageOptions.priority;
    }
    if (updateMessageOptions.attributes !== undefined) {
      this.attributes = updateMessageOptions.attributes;
    }
    if (updateMessageOptions.visibilityTimeoutMs != undefined) {
      this.expiresAt = new Date(now.getTime() + updateMessageOptions.visibilityTimeoutMs);
    }
  }

  public nak(now: Date, reason?: string): void {
    this.expiresAt = new Date(now.getTime() - 1);
    this.lastNakReason = reason;
    this._receiptHandle = undefined;
  }
}
