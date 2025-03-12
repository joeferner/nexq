import { Message } from "./Message.js";
import { Time } from "./Time.js";
import { User } from "./User.js";

export const DEFAULT_PASSWORD_HASH_ROUNDS = 10;
export const DEFAULT_MAX_RECEIVE_COUNT = 10;
export const DEFAULT_MAX_NUMBER_OF_MESSAGES = 10;

export interface CreateQueueOptions {
  /**
   * Optional dead letter queue in which messages that either have reached
   * their max receive count or have been nak'ed will be sent.
   */
  deadLetterQueueName?: string;

  /**
   * The length of time, for which the delivery of all messages in the queue
   * is delayed.
   */
  delayMs?: number;

  /**
   * The length of time, a messages is retained. Messages older than this
   * duration will be deleted.
   */
  messageRetentionPeriodMs?: number;

  /**
   * The visibility timeout for the queue. If a message visibility timeout
   * has not been extended in this period other clients will be allowed to
   * read the message
   */
  visibilityTimeoutMs?: number;

  /**
   * The length of time, for which a ReceiveMessage action waits for a
   * message to arrive.
   */
  receiveMessageWaitTimeMs?: number;

  /**
   * The length of time, for which this queue expires, if the queue expires
   * it and all it's messages are deleted
   */
  expiresMs?: number;

  /**
   * The number of times a message is delivered to the source queue before
   * being moved to the dead-letter queue.
   */
  maxReceiveCount?: number;

  /**
   * maximum size a message can be
   */
  maxMessageSize?: number;

  /**
   * tags to apply to this queue
   */
  tags?: Record<string, string>;
}

export interface SendMessageOptions {
  attributes?: Record<string, string>;

  /**
   * If set delays the message from being received by this amount
   */
  delayMs?: number;

  /**
   * if specified, higher priorities have higher priority (default: 0)
   */
  priority?: number;
}

export interface SendMessageResult {
  id: string;
  sequenceNumber: number;
}

export interface ReceiveMessageOptions {
  visibilityTimeoutMs?: number;
  waitTimeMs?: number;
}

export interface ReceiveMessagesOptions extends ReceiveMessageOptions {
  maxNumberOfMessages?: number;
}

export interface QueueInfo {
  name: string;
  numberOfMessages: number;
  numberOfMessagesDelayed: number;
  numberOfMessagesNotVisible: number;
  created: Date;
  lastModified: Date;
  delayMs?: number;
  expiresMs?: number;
  expiresAt?: Date;
  maxMessageSize?: number;
  messageRetentionPeriodMs?: number;
  receiveMessageWaitTimeMs?: number;
  visibilityTimeoutMs?: number;
  tags: Record<string, string>;
  deadLetter?: {
    queueName: string;
    maxReceiveCount: number;
  };
}

export interface UpdateMessageOptions {
  priority?: number;
  attributes?: Record<string, string>;
  visibilityTimeoutMs?: number;
}

export interface CreateUserOptions {
  username: string;
  password?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
}

export interface CreateTopicOptions {
  tags?: Record<string, string>;
}

export interface TopicInfo {
  name: string;
  subscriptions: TopicInfoSubscription[];
}

export interface TopicInfoQueueSubscription {
  id: string;
  protocol: TopicProtocol.Queue;
  queueName: string;
}

export type TopicInfoSubscription = TopicInfoQueueSubscription;

export enum TopicProtocol {
  Queue,
}

export abstract class Store {
  protected readonly _time: Time;

  protected constructor(time: Time) {
    this._time = time;
  }

  public abstract start(): Promise<void>;
  public abstract shutdown(): Promise<void>;

  public abstract createUser(options: CreateUserOptions): Promise<void>;
  public abstract getUserByUsername(username: string): Promise<User | undefined>;
  public abstract getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined>;
  public abstract getUsers(): Promise<User[]>;

  public async receiveMessage(queueName: string, options?: ReceiveMessageOptions): Promise<Message | undefined> {
    const messages = await this.receiveMessages(queueName, { ...options, maxNumberOfMessages: 1 });
    if (messages.length > 1) {
      throw new Error(`expected 0 or 1 but found ${messages.length} when receiving messages`);
    }
    return messages[0];
  }

  public abstract createQueue(queueName: string, options?: CreateQueueOptions): Promise<string>;
  public abstract sendMessage(
    queueName: string,
    body: string,
    options?: SendMessageOptions
  ): Promise<SendMessageResult>;
  public abstract receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<Message[]>;
  public abstract poll(): Promise<void>;
  public abstract changeMessageVisibilityByReceiptHandle(
    queueName: string,
    receiptHandle: string,
    timeMs: number
  ): Promise<void>;
  public abstract deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void>;
  public abstract getQueueInfo(queueName: string): Promise<QueueInfo>;
  public abstract getQueueInfos(): Promise<QueueInfo[]>;
  public abstract updateMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): Promise<void>;
  public abstract nakMessage(queueName: string, messageId: string, receiptHandle: string): Promise<void>;
  public abstract deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void>;
  public abstract deleteQueue(queueName: string): Promise<void>;
  public abstract purgeQueue(queueName: string): Promise<void>;

  public abstract getTopicInfos(): Promise<TopicInfo[]>;
  public abstract createTopic(topicName: string, options?: CreateTopicOptions): Promise<void>;
  public abstract subscribe(topicName: string, protocol: TopicProtocol, target: string): Promise<string>;
  public abstract deleteTopic(topicName: string): Promise<void>;
  public abstract publishMessage(
    topicName: string,
    body: string,
    options?: SendMessageOptions
  ): Promise<SendMessageResult>;
}
