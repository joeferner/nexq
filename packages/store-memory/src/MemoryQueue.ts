import {
  AbortError,
  createId,
  createLogger,
  CreateQueueOptions,
  DEFAULT_NAK_EXPIRE_BEHAVIOR,
  DEFAULT_VISIBILITY_TIMEOUT_MS,
  GetMessage,
  InvalidUpdateError,
  isDecreasePriorityByNakExpireBehavior,
  Message,
  MessageExceededMaxMessageSizeError,
  MessageNotFoundError,
  MoveMessagesResult,
  NakExpireBehaviorOptions,
  PeekMessagesOptions,
  QueueInfo,
  ReceiptHandleIsInvalidError,
  ReceivedMessage,
  ReceiveMessagesOptions,
  SendMessageOptions,
  SendMessageResult,
  Time,
  Trigger,
  UpdateMessageOptions,
} from "@nexq/core";
import * as R from "radash";
import { MemoryQueueMessage } from "./MemoryQueueMessage.js";
import { NewQueueMessageEvent, ResumeEvent } from "./events.js";

const logger = createLogger("MemoryStore:Queue");

export class MemoryQueue {
  public readonly name: string;
  public readonly created: Date;
  private readonly time: Time;
  public lastModified: Date;
  private delayMs?: number;
  public deadLetterQueueName: string | undefined;
  public deadLetterTopicName: string | undefined;
  private visibilityTimeoutMs: number | undefined;
  private receiveMessageWaitTimeMs: number | undefined;
  private messageRetentionPeriodMs: number | undefined;
  private expiresMs: number | undefined;
  private maxReceiveCount?: number;
  private maxMessageSize?: number;
  private tags: Record<string, string>;
  private nakExpireBehavior: NakExpireBehaviorOptions;
  private expiresAt: Date | undefined;
  private paused = false;
  private readonly messages: MemoryQueueMessage[] = [];
  private readonly triggers: Trigger<NewQueueMessageEvent | ResumeEvent>[] = [];

  public constructor(options: { name: string; time: Time } & CreateQueueOptions) {
    const now = options.time.getCurrentTime();
    this.name = options.name;
    this.time = options.time;
    this.created = now;
    this.lastModified = now;

    this.delayMs = options.delayMs;
    this.deadLetterQueueName = options.deadLetterQueueName;
    this.deadLetterTopicName = options.deadLetterTopicName;
    this.visibilityTimeoutMs = options.visibilityTimeoutMs;
    this.receiveMessageWaitTimeMs = options.receiveMessageWaitTimeMs;
    this.messageRetentionPeriodMs = options.messageRetentionPeriodMs;
    this.maxReceiveCount = options.maxReceiveCount;
    this.maxMessageSize = options.maxMessageSize;
    this.tags = options.tags ? structuredClone(options.tags) : {};
    this.nakExpireBehavior = options.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR;
    if (options.expiresMs !== undefined) {
      this.expiresMs = options.expiresMs;
      this.expiresAt = new Date(this.time.getCurrentTime().getTime() + options.expiresMs);
    }
  }

  public update(options: CreateQueueOptions): void {
    const now = this.time.getCurrentTime();
    this.delayMs = options.delayMs;
    this.deadLetterQueueName = options.deadLetterQueueName;
    this.deadLetterTopicName = options.deadLetterTopicName;
    this.visibilityTimeoutMs = options.visibilityTimeoutMs;
    this.receiveMessageWaitTimeMs = options.receiveMessageWaitTimeMs;
    this.messageRetentionPeriodMs = options.messageRetentionPeriodMs;
    this.maxReceiveCount = options.maxReceiveCount;
    this.maxMessageSize = options.maxMessageSize;
    this.tags = options.tags ? structuredClone(options.tags) : {};
    this.nakExpireBehavior = options.nakExpireBehavior ?? DEFAULT_NAK_EXPIRE_BEHAVIOR;
    if (options.expiresMs !== undefined) {
      this.expiresMs = options.expiresMs;
      this.expiresAt = new Date(now.getTime() + options.expiresMs);
    }
    this.lastModified = now;
  }

  public sendMessage(
    id: string | undefined,
    body: string,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): SendMessageResult {
    const now = this.time.getCurrentTime();
    id = id ?? createId();

    if (this.messages.some((m) => m.id === id)) {
      throw new Error(`duplicate message with id "${id}" in queue "${this.name}"`);
    }

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
        body,
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

  public pause(): void {
    this.paused = true;
  }

  public resume(): void {
    this.paused = false;
    this.trigger({ type: "resume" } satisfies ResumeEvent);
  }

  private trigger(message: NewQueueMessageEvent | ResumeEvent): void {
    const triggers = [...this.triggers];
    this.triggers.length = 0;
    for (const trigger of triggers) {
      trigger.trigger(message);
    }
  }

  public async receiveMessages(
    options?: ReceiveMessagesOptions & { maxNumberOfMessages: number }
  ): Promise<ReceivedMessage[]> {
    const visibilityTimeoutMs =
      options?.visibilityTimeoutMs ?? this.visibilityTimeoutMs ?? DEFAULT_VISIBILITY_TIMEOUT_MS;
    const waitTime = options?.waitTimeMs ?? this.receiveMessageWaitTimeMs ?? 0;
    const endTime = this.time.getCurrentTime().getTime() + waitTime;
    const messages: ReceivedMessage[] = [];
    while (true) {
      const now = this.time.getCurrentTime();

      if (this.expiresMs !== undefined) {
        this.expiresAt = new Date(now.getTime() + this.expiresMs);
      }

      if (!this.paused) {
        this.sortMessages();
        for (const message of this.messages) {
          if (this.maxReceiveCount !== undefined && message.receiveCount >= this.maxReceiveCount) {
            continue;
          }

          if (!message.isAvailable(now)) {
            continue;
          }

          const newExpiresAt = new Date(now.getTime() + visibilityTimeoutMs);
          messages.push(message.markReceived(newExpiresAt, now));
          if (messages.length === options?.maxNumberOfMessages) {
            return messages;
          }
        }
      }

      if (now.getTime() > endTime) {
        return messages;
      }

      const timeLeft = endTime - now.getTime();
      if (timeLeft > 0) {
        await this.sleepOrWaitForEvent(timeLeft, { signal: options?.abortSignal });
        if (options?.abortSignal?.aborted) {
          throw new AbortError("receive aborted");
        }
      } else {
        return messages;
      }
    }
  }

  public async peekMessages(options: Required<PeekMessagesOptions>): Promise<Message[]> {
    const messages: Message[] = [];

    const now = this.time.getCurrentTime();
    if (this.expiresMs !== undefined) {
      this.expiresAt = new Date(now.getTime() + this.expiresMs);
    }

    this.sortMessages();
    for (const message of this.messages) {
      if (options.includeNotVisible === false) {
        if (message.isNotVisible(now)) {
          continue;
        }
      }

      if (options.includeDelayed === false) {
        if (message.isDelayed(now)) {
          continue;
        }
      }

      messages.push(message.toMessage(now));

      if (messages.length === options.maxNumberOfMessages) {
        return messages;
      }
    }

    return messages;
  }

  public async getMessage(messageId: string): Promise<GetMessage> {
    const now = this.time.getCurrentTime();
    this.sortMessages();
    for (let i = 0; i < this.messages.length; i++) {
      const message = this.messages[i];
      if (message.id === messageId) {
        return message.toGetMessage(i, now);
      }
    }
    throw new MessageNotFoundError(this.name, messageId);
  }

  private async sleepOrWaitForEvent(ms: number, options?: { signal?: AbortSignal }): Promise<void> {
    const trigger = new Trigger<NewQueueMessageEvent>(this.time);
    this.triggers.push(trigger);
    await trigger.wait(ms, options);
  }

  private sortMessages(): void {
    this.messages.sort((a, b) => a.compareTo(b));
  }

  public getInfo(): QueueInfo {
    const now = this.time.getCurrentTime();
    let numberOfMessagesVisible = 0;
    let numberOfMessagesDelayed = 0;
    let numberOfMessagesNotVisible = 0;
    for (const message of this.messages) {
      if (message.isAvailable(now)) {
        numberOfMessagesVisible++;
      } else if (message.isDelayed(now)) {
        numberOfMessagesDelayed++;
      } else if (message.isNotVisible(now)) {
        numberOfMessagesNotVisible++;
      }
    }
    return {
      name: this.name,
      numberOfMessages: numberOfMessagesVisible + numberOfMessagesDelayed + numberOfMessagesNotVisible,
      numberOfMessagesVisible,
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
      nakExpireBehavior: R.clone(this.nakExpireBehavior),
      tags: structuredClone(this.tags),
      deadLetterQueueName: this.deadLetterQueueName,
      deadLetterTopicName: this.deadLetterTopicName,
      maxReceiveCount: this.maxReceiveCount,
      paused: this.paused,
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

      if (message.expiresAt && now > message.expiresAt) {
        message.expiresAt = undefined;
        if (this.maxReceiveCount && message.receiveCount >= this.maxReceiveCount) {
          expiredMessages.push(message);
          this.messages.splice(i, 1);
          continue;
        } else {
          if (this.nakExpireBehavior === "retry") {
            // do nothing
          } else if (this.nakExpireBehavior === "moveToEnd") {
            message.orderTime = now;
          } else if (isDecreasePriorityByNakExpireBehavior(this.nakExpireBehavior)) {
            message.priority -= this.nakExpireBehavior.decreasePriorityBy;
          } else {
            throw new Error(`unhandled nakExpireBehavior ${JSON.stringify(this.nakExpireBehavior)}`);
          }
        }
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
