import { CreateQueueOptions, NakExpireBehaviorOptions } from "./dto/CreateQueueOptions.js";
import { CreateTopicOptions } from "./dto/CreateTopicOptions.js";
import { CreateUserOptions } from "./dto/CreateUserOptions.js";
import { DeleteMessagesMessage } from "./dto/DeleteMessagesMessage.js";
import { DeleteMessagesResult } from "./dto/DeleteMessagesResult.js";
import { MoveMessagesResult } from "./dto/MoveMessagesResult.js";
import { PeekMessagesOptions } from "./dto/PeekMessagesOptions.js";
import { QueueInfo } from "./dto/QueueInfo.js";
import { ReceiveMessageOptions } from "./dto/ReceiveMessageOptions.js";
import { ReceiveMessagesOptions } from "./dto/ReceiveMessagesOptions.js";
import { SendMessageOptions } from "./dto/SendMessageOptions.js";
import { SendMessageResult } from "./dto/SendMessageResult.js";
import { SendMessagesOptions } from "./dto/SendMessagesOptions.js";
import { SendMessagesResult } from "./dto/SendMessagesResult.js";
import { TopicInfo } from "./dto/TopicInfo.js";
import { TopicProtocol } from "./dto/TopicInfoSubscription.js";
import { UpdateMessageOptions } from "./dto/UpdateMessageOptions.js";
import { GetMessage, Message, ReceivedMessage } from "./Message.js";
import { User } from "./User.js";

export const DEFAULT_PASSWORD_HASH_ROUNDS = 10;
export const DEFAULT_MAX_NUMBER_OF_MESSAGES = 10;
export const DEFAULT_NAK_EXPIRE_BEHAVIOR: NakExpireBehaviorOptions = "retry";
export const DEFAULT_VISIBILITY_TIMEOUT_MS = 60 * 1000;

export interface Store {
  shutdown(): Promise<void>;

  createUser(options: CreateUserOptions): Promise<void>;
  getUserByUsername(username: string): Promise<User | undefined>;
  getUserByAccessKeyId(accessKeyId: string): Promise<User | undefined>;
  getUsers(): Promise<User[]>;

  createQueue(queueName: string, options?: CreateQueueOptions): Promise<void>;
  sendMessage(queueName: string, body: string, options?: SendMessageOptions): Promise<SendMessageResult>;
  sendMessages(queueName: string, options: SendMessagesOptions): Promise<SendMessagesResult>;
  receiveMessage(queueName: string, options?: ReceiveMessageOptions): Promise<ReceivedMessage | undefined>;
  receiveMessages(queueName: string, options?: ReceiveMessagesOptions): Promise<ReceivedMessage[]>;
  peekMessages(queueName: string, options?: PeekMessagesOptions): Promise<Message[]>;
  getMessage(queueName: string, messageId: string): Promise<GetMessage>;
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
  nakMessage(queueName: string, messageId: string, receiptHandle: string, reason?: string): Promise<void>;
  deleteMessage(queueName: string, messageId: string, receiptHandle?: string): Promise<void>;
  deleteMessages(queueName: string, messages: DeleteMessagesMessage[]): Promise<DeleteMessagesResult>;
  deleteQueue(queueName: string): Promise<void>;
  purgeQueue(queueName: string): Promise<void>;
  moveMessages(sourceQueueName: string, targetQueueName: string): Promise<MoveMessagesResult>;
  pause(queueName: string): Promise<void>;
  resume(queueName: string): Promise<void>;

  getTopicInfo(topicName: string): Promise<TopicInfo>;
  getTopicInfos(): Promise<TopicInfo[]>;
  createTopic(topicName: string, options?: CreateTopicOptions): Promise<void>;
  subscribe(topicName: string, protocol: TopicProtocol, target: string): Promise<string>;
  deleteSubscription(subscriptionId: string): Promise<void>;
  deleteTopic(topicName: string): Promise<void>;
  publishMessage(topicName: string, body: string, options?: SendMessageOptions): Promise<void>;
}
