import {
  CreateQueueOptions,
  CreateTopicOptions,
  Message,
  QueueInfo,
  SendMessageOptions,
  SendMessageResult,
  TopicInfo,
  User,
} from "@nexq/core";

export interface Dialect {
  shutdown(): Promise<void>;

  createTransaction(): Promise<Transaction>;

  findUserByAccessKeyId(tx: Transaction | undefined, accessKeyId: string): Promise<User | undefined>;
  findUserByUsername(tx: Transaction | undefined, username: string): Promise<User | undefined>;
  createUser(tx: Transaction, options: DialectCreateUser): Promise<void>;
  findUsers(tx: Transaction | undefined): Promise<User[]>;

  createQueue(tx: Transaction, queueName: string, options?: CreateQueueOptions): Promise<void>;
  getQueueInfo(tx: Transaction | undefined, queueName: string): Promise<QueueInfo | undefined>;
  getQueueInfos(tx: Transaction | undefined): Promise<QueueInfo[]>;
  sendMessage(queueInfo: QueueInfo, body: Buffer, options: SendMessageOptions | undefined): Promise<SendMessageResult>;
  receiveMessages(queueName: string, options: { visibilityTimeoutMs: number; count: number }): Promise<Message[]>;
  updateMessageVisibilityByReceiptHandle(queueName: string, receiptHandle: string, timeMs: number): Promise<void>;
  deleteMessageByReceiptHandle(queueName: string, receiptHandle: string): Promise<void>;

  deleteExpiredMessages(queueInfo: QueueInfo): Promise<void>;
  moveExpiredMessagesToDeadLetter(queueInfo: QueueInfo, deadLetterQueueInfo: QueueInfo): Promise<void>;

  getTopicInfos(tx: Transaction | undefined): Promise<TopicInfo[]>;
  getTopicInfo(tx: Transaction | undefined, topicName: string): Promise<TopicInfo | undefined>;
  createTopic(tx: Transaction, topicName: string, options?: CreateTopicOptions): Promise<void>;
  subscribe(tx: Transaction, id: string, topicName: string, queueName: string): Promise<void>;
}

export interface Transaction {
  rollback(): Promise<void>;
  commit(): Promise<void>;
}

export interface DialectCreateUser {
  id: string;
  username: string;
  passwordHash: string | null;
  accessKeyId: string | null;
  secretAccessKey: string | null;
}
