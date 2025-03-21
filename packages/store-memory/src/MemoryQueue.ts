import {
  createId,
  createLogger,
  CreateQueueOptions,
  InvalidUpdateError,
  Message,
  MessageExceededMaxMessageSizeError,
  MessageNotFoundError,
  MoveMessagesResult,
  QueueInfo,
  ReceiptHandleIsInvalidError,
  ReceiveMessagesOptions,
  SendMessageOptions,
  SendMessageResult,
  Time,
  Trigger,
  UpdateMessageOptions,
} from "@nexq/core";
import * as R from "radash";
import { MemoryQueueMessage } from "./MemoryQueueMessage.js";
import { NewQueueMessageEvent } from "./events.js";

const logger = createLogger("MemoryQueue");

export class MemoryQueue {
  public readonly name: string;
  public readonly created: Date;
  public readonly lastModified: Date;
  private readonly time: Time;
  private readonly delayMs?: number;
  public readonly deadLetterQueueName: string | undefined;
  private readonly visibilityTimeoutMs: number | undefined;
  private readonly receiveMessageWaitTimeMs: number | undefined;
  private readonly messageRetentionPeriodMs: number | undefined;
  private readonly expiresMs: number | undefined;
  private readonly messages: MemoryQueueMessage[] = [];
  private readonly maxReceiveCount?: number;
  private readonly maxMessageSize?: number;
  private readonly tags: Record<string, string>;
  private readonly triggers: Trigger<NewQueueMessageEvent>[] = [];
  private expiresAt: Date | undefined;

  public constructor(options: { name: string; time: Time } & CreateQueueOptions) {
    const now = options.time.getCurrentTime();
    this.name = options.name;
    this.time = options.time;
    this.created = now;
    this.lastModified = now;
    this.delayMs = options.delayMs;
    this.deadLetterQueueName = options.deadLetterQueueName;
    this.visibilityTimeoutMs = options.visibilityTimeoutMs;
    this.receiveMessageWaitTimeMs = options.receiveMessageWaitTimeMs;
    this.messageRetentionPeriodMs = options.messageRetentionPeriodMs;
    this.maxReceiveCount = options.maxReceiveCount;
    this.maxMessageSize = options.maxMessageSize;
    this.tags = options.tags ? structuredClone(options.tags) : {};
    if (options.expiresMs !== undefined) {
      this.expiresMs = options.expiresMs;
      this.expiresAt = new Date(options.time.getCurrentTime().getTime() + options.expiresMs);
    }
  }

  public sendMessage(
    body: string | Buffer,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): SendMessageResult {
    const now = this.time.getCurrentTime();
    const id = createId();
    const delay = options?.delayMs ?? this.delayMs;
    const delayUntil = delay === undefined ? undefined : new Date(now.getTime() + delay);
    if (this.maxMessageSize && body.length > this.maxMessageSize) {
      throw new MessageExceededMaxMessageSizeError(body.length, this.maxMessageSize);
    }
    this.messages.push(
      new MemoryQueueMessage({
        id,
        priority: options?.priority ?? 0,
        sentTime: now,
        attributes: options?.attributes ?? {},
        body: R.isString(body) ? Buffer.from(body) : body,
        delayUntil,
        retainUntil:
          this.messageRetentionPeriodMs === undefined
            ? undefined
            : new Date(now.getTime() + this.messageRetentionPeriodMs),
        lastNakReason: options?.lastNakReason,
      })
    );
    this.trigger({ type: "new-queue-message", queueName: this.name } satisfies NewQueueMessageEvent);
    return { id };
  }

  private trigger(message: NewQueueMessageEvent): void {
    const triggers = [...this.triggers];
    this.triggers.length = 0;
    for (const trigger of triggers) {
      trigger.trigger(message);
    }
  }

  public async receiveMessages(options?: ReceiveMessagesOptions & { maxNumberOfMessages: number }): Promise<Message[]> {
    const visibilityTimeoutMs = options?.visibilityTimeoutMs ?? this.visibilityTimeoutMs ?? 60;
    const waitTime = options?.waitTimeMs ?? this.receiveMessageWaitTimeMs ?? 0;
    const endTime = this.time.getCurrentTime().getTime() + waitTime;
    const messages: Message[] = [];
    while (true) {
      const now = this.time.getCurrentTime();

      if (this.expiresMs !== undefined) {
        this.expiresAt = new Date(now.getTime() + this.expiresMs);
      }

      this.sortMessages();
      for (const message of this.messages) {
        if (!message.isAvailable(now)) {
          continue;
        }

        const newExpiresAt = new Date(now.getTime() + visibilityTimeoutMs);
        messages.push(message.markReceived(newExpiresAt, now));
        if (messages.length === options?.maxNumberOfMessages) {
          return messages;
        }
      }

      if (now.getTime() > endTime) {
        return messages;
      }

      const timeLeft = endTime - now.getTime();
      if (timeLeft > 0) {
        await this.sleepOrWaitForEvent(timeLeft);
      } else {
        return messages;
      }
    }
  }

  private async sleepOrWaitForEvent(ms: number): Promise<void> {
    const trigger = new Trigger<NewQueueMessageEvent>(this.time);
    this.triggers.push(trigger);
    await trigger.wait(ms);
  }

  private sortMessages(): void {
    this.messages.sort((a, b) => a.compareTo(b));
  }

  public getInfo(): QueueInfo {
    const now = this.time.getCurrentTime();
    let numberOfMessages = 0;
    let numberOfMessagesDelayed = 0;
    let numberOfMessagesNotVisible = 0;
    for (const message of this.messages) {
      if (message.isAvailable(now)) {
        numberOfMessages++;
      } else if (message.isDelayed(now)) {
        numberOfMessagesDelayed++;
      } else if (message.isNotVisible(now)) {
        numberOfMessagesNotVisible++;
      }
    }
    return {
      name: this.name,
      numberOfMessages,
      numberOfMessagesDelayed,
      numberOfMessagesNotVisible,
      created: this.created,
      lastModified: this.lastModified,
      delayMs: this.delayMs,
      expiresMs: this.expiresMs,
      expiresAt: this.expiresAt,
      maxMessageSize: this.maxMessageSize,
      messageRetentionPeriodMs: this.messageRetentionPeriodMs,
      receiveMessageWaitTimeMs: this.receiveMessageWaitTimeMs,
      visibilityTimeoutMs: this.visibilityTimeoutMs,
      tags: structuredClone(this.tags),
      deadLetterQueueName: this.deadLetterQueueName,
      maxReceiveCount: this.maxReceiveCount,
    };
  }

  public isExpired(now: Date): boolean {
    if (this.expiresAt) {
      return now > this.expiresAt;
    }
    return false;
  }

  public poll(now: Date): MemoryQueueMessage[] {
    const expiredMessages: MemoryQueueMessage[] = [];
    for (let i = this.messages.length - 1; i >= 0; i--) {
      const message = this.messages[i];

      if (message.retainUntil) {
        if (now > message.retainUntil) {
          logger.debug(`deleting message ${message.id}, exceeded message retention period`);
          this.messages.splice(i, 1);
          continue;
        }
      }

      if (
        message.expiresAt &&
        this.maxReceiveCount &&
        now > message.expiresAt &&
        message.receiveCount >= this.maxReceiveCount
      ) {
        expiredMessages.push(message);
        this.messages.splice(i, 1);
        continue;
      }
    }
    return expiredMessages;
  }

  public updateMessageVisibilityByReceiptHandle(receiptHandle: string, expiresAt: Date): void {
    const message = this.messages.find((m) => m.receiptHandle === receiptHandle);
    if (!message) {
      throw new ReceiptHandleIsInvalidError(this.name, receiptHandle);
    }
    message.expiresAt = expiresAt;
  }

  public deleteMessageByReceiptHandle(receiptHandle: string): MemoryQueueMessage[] {
    return this.deleteMessageIf((m) => m.receiptHandle === receiptHandle);
  }

  private deleteMessageIf(fn: (m: MemoryQueueMessage) => boolean): MemoryQueueMessage[] {
    const deletedMessages: MemoryQueueMessage[] = [];
    for (let i = this.messages.length - 1; i >= 0; i--) {
      const message = this.messages[i];
      if (fn(message)) {
        deletedMessages.push(message);
        this.messages.splice(i, 1);
        continue;
      }
    }
    return deletedMessages;
  }

  public updateMessage(
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): void {
    const now = this.time.getCurrentTime();
    const message = this.messages.find((m) => m.id === messageId);
    if (!message) {
      throw new MessageNotFoundError(this.name, messageId);
    }
    if (receiptHandle === undefined && updateMessageOptions.visibilityTimeoutMs !== undefined) {
      throw new InvalidUpdateError("cannot update message visibility timeout without providing a receipt handle");
    }
    if (receiptHandle !== undefined && message.receiptHandle !== receiptHandle) {
      throw new ReceiptHandleIsInvalidError(this.name, receiptHandle);
    }
    message.update(now, updateMessageOptions);
  }

  public nakMessage(messageId: string, receiptHandle: string, reason?: string): void {
    const now = this.time.getCurrentTime();
    const message = this.messages.find((m) => m.id === messageId);
    if (!message) {
      throw new MessageNotFoundError(this.name, messageId);
    }
    if (message.receiptHandle !== receiptHandle) {
      throw new ReceiptHandleIsInvalidError(this.name, receiptHandle);
    }
    message.nak(now, reason);
  }

  public deleteMessage(id: string, receiptHandle?: string): void {
    const messages = this.deleteMessageIf((m) => {
      if (m.id === id) {
        if (receiptHandle !== undefined && m.receiptHandle !== receiptHandle) {
          throw new ReceiptHandleIsInvalidError(this.name, receiptHandle);
        }
        return true;
      }
      return false;
    });
    if (messages.length === 0) {
      throw new MessageNotFoundError(this.name, id);
    }
  }

  public purge(): void {
    this.messages.length = 0;
  }

  public moveMessages(targetQueue: MemoryQueue): MoveMessagesResult {
    let movedMessageCount = 0;
    const now = this.time.getCurrentTime();
    for (let i = this.messages.length - 1; i >= 0; i--) {
      const message = this.messages[i];
      if (!message.isAvailable(now)) {
        continue;
      }

      this.messages.splice(i, 1);
      movedMessageCount++;
      message.retainUntil =
        targetQueue.messageRetentionPeriodMs === undefined
          ? undefined
          : new Date(now.getTime() + targetQueue.messageRetentionPeriodMs);
      targetQueue.messages.push(message);
    }
    return { movedMessageCount };
  }
}
