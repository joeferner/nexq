import * as R from "radash";
import { Message } from "./Message.js";
import { User } from "./User.js";

export const DEFAULT_PASSWORD_HASH_ROUNDS = 10;
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
  deadLetterQueueName?: string;
  maxReceiveCount?: number;
}

export function queueInfoEqualCreateQueueOptions(queueInfo: QueueInfo, options: CreateQueueOptions): boolean {
  if (queueInfo.deadLetterQueueName !== options.deadLetterQueueName) {
    return false;
  }
  if (queueInfo.delayMs !== options.delayMs) {
    return false;
  }
  if (queueInfo.messageRetentionPeriodMs !== options.messageRetentionPeriodMs) {
    return false;
  }
  if (queueInfo.visibilityTimeoutMs !== options.visibilityTimeoutMs) {
    return false;
  }
  if (queueInfo.receiveMessageWaitTimeMs !== options.receiveMessageWaitTimeMs) {
    return false;
  }
  if (queueInfo.expiresMs !== options.expiresMs) {
    return false;
  }
  if (queueInfo.maxReceiveCount !== options.maxReceiveCount) {
    return false;
  }
  if (queueInfo.maxMessageSize !== options.maxMessageSize) {
    return false;
  }
  if (!R.isEqual(queueInfo.tags, options.tags)) {
    return false;
  }
  return true;
}

export function topicInfoEqualCreateTopicOptions(topicInfo: TopicInfo, options: CreateTopicOptions): boolean {
  return R.isEqual(topicInfo.tags, options.tags);
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
  tags: Record<string, string>;
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

export interface Store {
  shutdown(): Promise<void>;

  createUser(options: CreateUserOptions): Promise<void>;
  getUserByUsername(username: string): Promise<User | undefined>;
  getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined>;
  getUsers(): Promise<User[]>;

  receiveMessage(queueName: string, options?: ReceiveMessageOptions): Promise<Message | undefined>;

  createQueue(queueName: string, options?: CreateQueueOptions): Promise<void>;
  sendMessage(queueName: string, body: string | Buffer, options?: SendMessageOptions): Promise<SendMessageResult>;
  receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<Message[]>;
  poll(): Promise<void>;
  updateMessageVisibilityByReceiptHandle(queueName: string, receiptHandle: string, timeMs: number): Promise<void>;
  deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void>;
  getQueueInfo(queueName: string): Promise<QueueInfo>;
  getQueueInfos(): Promise<QueueInfo[]>;
  updateMessage(
    queueName: string,
    messageId: string,
    receiptHandle: string | undefined,
    updateMessageOptions: UpdateMessageOptions
  ): Promise<void>;
  nakMessage(queueName: string, messageId: string, receiptHandle: string): Promise<void>;
  deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void>;
  deleteQueue(queueName: string): Promise<void>;
  purgeQueue(queueName: string): Promise<void>;

  getTopicInfo(topicName: string): Promise<TopicInfo>;
  getTopicInfos(): Promise<TopicInfo[]>;
  createTopic(topicName: string, options?: CreateTopicOptions): Promise<void>;
  subscribe(topicName: string, protocol: TopicProtocol, target: string): Promise<string>;
  deleteTopic(topicName: string): Promise<void>;
  publishMessage(topicName: string, body: string, options?: SendMessageOptions): Promise<SendMessageResult>;
}
