import { createId, Message, UpdateMessageOptions } from "@nexq/core";

export interface CreateMemoryQueueMessageOptions {
  id: string;
  sequenceNumber: number;
  priority: number;
  sentTime: Date;
  attributes: Record<string, string>;
  body: Buffer;
  delayUntil: Date | undefined;
}

export class MemoryQueueMessage {
  public readonly id: string;
  public readonly body: Buffer;
  public attributes: Record<string, string>;
  public priority: number;
  /**
   * used with visibility timeout to determine how long the message is taken by the caller
   */
  public takenUntil?: Date;
  private _receiveCount = 0;
  private _receiptHandle?: string;
  private _firstReceiveTime?: Date;
  private _delayUntil?: Date;
  /**
   * Time the message was originally sent
   */
  public readonly sentTime: Date;

  public constructor(options: CreateMemoryQueueMessageOptions) {
    this.id = options.id;
    this.body = options.body;
    this.attributes = options.attributes;
    this.sentTime = options.sentTime;
    this.priority = options.priority;
    this._delayUntil = options.delayUntil;
  }

  public get receiptHandle(): string | undefined {
    return this._receiptHandle;
  }

  public get receiveCount(): number {
    return this._receiveCount;
  }

  public markReceived(newTakenUntil: Date, now: Date): Message {
    this.takenUntil = newTakenUntil;
    this._receiptHandle = createId();
    if (this._firstReceiveTime === undefined) {
      this._firstReceiveTime = now;
    }
    this._receiveCount++;
    return new Message({
      id: this.id,
      receiptHandle: this._receiptHandle,
      body: this.body,
      sentTime: this.sentTime,
      priority: this.priority,
      attributes: this.attributes,
    });
  }

  public isAvailable(now: Date): boolean {
    if (this.takenUntil !== undefined) {
      if (this.takenUntil >= now) {
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

    return this.sentTime.getTime() - other.sentTime.getTime();
  }

  public update(now: Date, updateMessageOptions: UpdateMessageOptions): void {
    if (updateMessageOptions.priority !== undefined) {
      this.priority = updateMessageOptions.priority;
    }
    if (updateMessageOptions.attributes !== undefined) {
      this.attributes = updateMessageOptions.attributes;
    }
    if (updateMessageOptions.visibilityTimeoutMs != undefined) {
      this.takenUntil = new Date(now.getTime() + updateMessageOptions.visibilityTimeoutMs);
    }
  }

  public nak(now: Date): void {
    this.takenUntil = new Date(now.getTime() - 1);
  }
}
