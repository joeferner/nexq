/* eslint-disable */
/* tslint:disable */
/*
 * ---------------------------------------------------------------
 * ## THIS FILE WAS GENERATED VIA SWAGGER-TYPESCRIPT-API        ##
 * ##                                                           ##
 * ## AUTHOR: acacode                                           ##
 * ## SOURCE: https://github.com/acacode/swagger-typescript-api ##
 * ---------------------------------------------------------------
 */

/** Construct a type with a set of properties K of type T */
export type RecordStringString = Record<string, string>;

export interface CreateTopicRequest {
  /**
   * The name of the topic
   * @example "my-topic"
   */
  name: string;
  /** tags to apply to this topic */
  tags?: RecordStringString;
}

export enum TopicProtocolQueue {
  Queue = "Queue",
}

export interface TopicInfoQueueSubscription {
  id: string;
  protocol: TopicProtocolQueue;
  queueName: string;
}

export type TopicInfoSubscription = TopicInfoQueueSubscription;

export interface GetTopicResponse {
  name: string;
  /** Construct a type with a set of properties K of type T */
  tags: RecordStringString;
  subscriptions: TopicInfoSubscription[];
}

export interface GetTopicsResponse {
  topics: GetTopicResponse[];
}

export interface SendMessageRequest {
  /**
   * The body of the queue message
   * @example "test message"
   */
  body: string;
  /**
   * delay before the message can be delivered
   * @example "5s"
   */
  delay?: string;
  /**
   * priority to give the message, higher priority messages will be delivered first
   * @format double
   * @example "5"
   */
  priority?: number;
  /** attributes to include with the message */
  attributes?: RecordStringString;
}

export interface SubscribeResponse {
  /** id of the subscription */
  id: string;
}

export interface SubscribeQueueRequest {
  /**
   * name of the queue to subscribe
   * @example "queue1"
   */
  queueName: string;
}

export interface DecreasePriorityByNakExpireBehavior {
  /** @format double */
  decreasePriorityBy: number;
}

export type NakExpireBehaviorOptions =
  | DecreasePriorityByNakExpireBehavior
  | "retry"
  | "moveToEnd"
  | ((DecreasePriorityByNakExpireBehavior & "retry") | "moveToEnd");

export interface CreateQueueRequest {
  /** If true the queue will either be updated or created (default: false) */
  upsert?: boolean;
  /**
   * The name of the queue
   * @example "my-queue"
   */
  name: string;
  /**
   * the default delay on the queue in seconds
   * @example "10ms"
   */
  delay?: string;
  /**
   * The length of time, for which this queue expires, if the queue expires it and all it's messages are deleted
   * @example "1d"
   */
  expires?: string;
  /**
   * the limit of how many bytes a message can contain
   * @example "10mb"
   */
  maximumMessageSize?: any;
  /**
   * the length of time, retains a message
   * @example "1d"
   */
  messageRetentionPeriod?: string;
  /**
   * the length of time, for which the ReceiveMessage action waits for a message to arrive
   * @example "10s"
   */
  receiveMessageWaitTime?: string;
  /**
   * The visibility timeout for the queue. If a message visibility timeout has not been extended in this
   * period other clients will be allowed to read the message
   * @example "1m"
   */
  visibilityTimeout?: string;
  /**
   * name of the dead letter queue
   * @example "my-queue-dlq"
   */
  deadLetterQueueName?: string;
  /**
   * name of the dead letter topic
   * @example "my-topic-dlq"
   */
  deadLetterTopicName?: string;
  /**
   * max number of time to receive a message before moving it to the dlq
   * @format double
   * @example "10"
   */
  maxReceiveCount?: number;
  /**
   * Determines the behavior of messages when they are either nak'ed or have
   * expired visibility timeout. If the message has exceeded it's maxReceiveCount
   * the message will be moved to the dead letter queue irregardless of this
   * setting.
   *
   * retry     - the message is kept at the same position (default)
   * moveToEnd - the message is moved to the end of the queue
   * { decreasePriorityBy: number } - decrease the priority of messages by the given amount
   */
  nakExpireBehavior?: NakExpireBehaviorOptions;
  /** tags to apply to this queue */
  tags?: RecordStringString;
}

export interface GetQueueResponse {
  name: string;
  /** @format double */
  numberOfMessage: number;
  /** @format double */
  numberOfMessagesVisible: number;
  /** @format double */
  numberOfMessagesNotVisible: number;
  /** @format double */
  numberOfMessagesDelayed: number;
  /** @format date-time */
  created: string;
  /** @format date-time */
  lastModified: string;
  /** @format double */
  delayMs?: number;
  /** @format double */
  expiresMs?: number;
  /** @format date-time */
  expiresAt?: string;
  /** @format double */
  maxMessageSize?: number;
  /** @format double */
  messageRetentionPeriodMs?: number;
  /** @format double */
  receiveMessageWaitTimeMs?: number;
  /** @format double */
  visibilityTimeoutMs?: number;
  /** Construct a type with a set of properties K of type T */
  tags: RecordStringString;
  deadLetterQueueName?: string;
  deadLetterTopicName?: string;
  /** @format double */
  maxReceiveCount?: number;
  paused: boolean;
}

export interface GetQueuesResponse {
  queues: GetQueueResponse[];
}

export interface SendMessageResponse {
  id: string;
}

export interface MoveMessagesResponse {
  /** @format double */
  movedMessageCount: number;
}

export interface UpdateMessageRequest {
  /**
   * New priority for the message
   * @format double
   * @example 5
   */
  priority?: number;
  /** New attributes for the message, this will replace any existing attributes */
  attributes?: RecordStringString;
  /**
   * The visibility timeout for the message. If a message visibility timeout has not been extended in this
   * period other clients will be allowed to read the message.
   *
   * This value can only be set when using a receipt handle.
   *
   * Setting this value to zero is the same as nak'ing the message.
   * @example "1m"
   */
  visibilityTimeout?: string;
}

export interface ReceiveMessagesResponseMessage {
  id: string;
  receiptHandle: string;
  body: string;
  /** @format double */
  priority: number;
  /** Construct a type with a set of properties K of type T */
  attributes: RecordStringString;
  /** Time the message was originally sent */
  sentTime: string;
}

export interface ReceiveMessagesResponse {
  messages: ReceiveMessagesResponseMessage[];
}

export interface ReceiveMessagesRequest {
  /** @format double */
  maxNumberOfMessages?: number;
  visibilityTimeout?: string;
  waitTime?: string;
}

export interface PeekMessagesResponseMessage {
  id: string;
  body: string;
  /** @format double */
  priority: number;
  /** Construct a type with a set of properties K of type T */
  attributes: RecordStringString;
  /** Time the message was originally sent */
  sentTime: string;
}

export interface PeekMessagesResponse {
  messages: PeekMessagesResponseMessage[];
}

export interface GetMessageResponse {
  id: string;
  body: string;
  /** @format double */
  priority: number;
  /** Construct a type with a set of properties K of type T */
  attributes: RecordStringString;
  /** Time the message was originally sent */
  sentTime: string;
  /** @format double */
  positionInQueue: number;
  delayUntil?: string;
  isAvailable: boolean;
  /** @format double */
  receiveCount: number;
  expiresAt?: string;
  receiptHandle?: string;
  firstReceivedAt?: string;
  lastNakReason?: string;
}

import axios, { AxiosInstance, AxiosRequestConfig, AxiosResponse, HeadersDefaults, ResponseType } from "axios";

export type QueryParamsType = Record<string | number, any>;

export interface FullRequestParams extends Omit<AxiosRequestConfig, "data" | "params" | "url" | "responseType"> {
  /** set parameter to `true` for call `securityWorker` for this request */
  secure?: boolean;
  /** request path */
  path: string;
  /** content type of request body */
  type?: ContentType;
  /** query params */
  query?: QueryParamsType;
  /** format of response (i.e. response.json() -> format: "json") */
  format?: ResponseType;
  /** request body */
  body?: unknown;
}

export type RequestParams = Omit<FullRequestParams, "body" | "method" | "query" | "path">;

export interface ApiConfig<SecurityDataType = unknown> extends Omit<AxiosRequestConfig, "data" | "cancelToken"> {
  securityWorker?: (
    securityData: SecurityDataType | null
  ) => Promise<AxiosRequestConfig | void> | AxiosRequestConfig | void;
  secure?: boolean;
  format?: ResponseType;
}

export enum ContentType {
  Json = "application/json",
  FormData = "multipart/form-data",
  UrlEncoded = "application/x-www-form-urlencoded",
  Text = "text/plain",
}

export class HttpClient<SecurityDataType = unknown> {
  public instance: AxiosInstance;
  private securityData: SecurityDataType | null = null;
  private securityWorker?: ApiConfig<SecurityDataType>["securityWorker"];
  private secure?: boolean;
  private format?: ResponseType;

  constructor({ securityWorker, secure, format, ...axiosConfig }: ApiConfig<SecurityDataType> = {}) {
    this.instance = axios.create({ ...axiosConfig, baseURL: axiosConfig.baseURL || "/" });
    this.secure = secure;
    this.format = format;
    this.securityWorker = securityWorker;
  }

  public setSecurityData = (data: SecurityDataType | null) => {
    this.securityData = data;
  };

  protected mergeRequestParams(params1: AxiosRequestConfig, params2?: AxiosRequestConfig): AxiosRequestConfig {
    const method = params1.method || (params2 && params2.method);

    return {
      ...this.instance.defaults,
      ...params1,
      ...(params2 || {}),
      headers: {
        ...((method && this.instance.defaults.headers[method.toLowerCase() as keyof HeadersDefaults]) || {}),
        ...(params1.headers || {}),
        ...((params2 && params2.headers) || {}),
      },
    };
  }

  protected stringifyFormItem(formItem: unknown) {
    if (typeof formItem === "object" && formItem !== null) {
      return JSON.stringify(formItem);
    } else {
      return `${formItem}`;
    }
  }

  protected createFormData(input: Record<string, unknown>): FormData {
    return Object.keys(input || {}).reduce((formData, key) => {
      const property = input[key];
      const propertyContent: any[] = property instanceof Array ? property : [property];

      for (const formItem of propertyContent) {
        const isFileType = formItem instanceof Blob || formItem instanceof File;
        formData.append(key, isFileType ? formItem : this.stringifyFormItem(formItem));
      }

      return formData;
    }, new FormData());
  }

  public request = async <T = any, _E = any>({
    secure,
    path,
    type,
    query,
    format,
    body,
    ...params
  }: FullRequestParams): Promise<AxiosResponse<T>> => {
    const secureParams =
      ((typeof secure === "boolean" ? secure : this.secure) &&
        this.securityWorker &&
        (await this.securityWorker(this.securityData))) ||
      {};
    const requestParams = this.mergeRequestParams(params, secureParams);
    const responseFormat = format || this.format || undefined;

    if (type === ContentType.FormData && body && body !== null && typeof body === "object") {
      body = this.createFormData(body as Record<string, unknown>);
    }

    if (type === ContentType.Text && body && body !== null && typeof body !== "string") {
      body = JSON.stringify(body);
    }

    return this.instance.request({
      ...requestParams,
      headers: {
        ...(requestParams.headers || {}),
        ...(type && type !== ContentType.FormData ? { "Content-Type": type } : {}),
      },
      params: query,
      responseType: responseFormat,
      data: body,
      url: path,
    });
  };
}

/**
 * @title @nexq/proto-rest
 * @version 1.0.0
 * @license MIT
 * @baseUrl /
 * @contact Joe Ferner  <joe@fernsroth.com>
 *
 * NexQ REST Protocol
 */
export class NexqApi<SecurityDataType extends unknown> extends HttpClient<SecurityDataType> {
  api = {
    /**
     * @description create a topic
     *
     * @tags topic
     * @name CreateTopic
     * @request POST:/api/v1/topic
     */
    createTopic: (data: CreateTopicRequest, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/topic`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        ...params,
      }),

    /**
     * @description gets a list of all topics
     *
     * @tags topic
     * @name GetTopics
     * @request GET:/api/v1/topic
     */
    getTopics: (params: RequestParams = {}) =>
      this.request<GetTopicsResponse, any>({
        path: `/api/v1/topic`,
        method: "GET",
        format: "json",
        ...params,
      }),

    /**
     * @description get topic info
     *
     * @tags topic
     * @name GetTopic
     * @request GET:/api/v1/topic/{topicName}
     */
    getTopic: (topicName: string, params: RequestParams = {}) =>
      this.request<GetTopicResponse, void>({
        path: `/api/v1/topic/${topicName}`,
        method: "GET",
        format: "json",
        ...params,
      }),

    /**
     * @description delete a topic
     *
     * @tags topic
     * @name DeleteTopic
     * @request DELETE:/api/v1/topic/{topicName}
     */
    deleteTopic: (topicName: string, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/topic/${topicName}`,
        method: "DELETE",
        ...params,
      }),

    /**
     * @description publish a message to a topic
     *
     * @tags topic
     * @name Publish
     * @request POST:/api/v1/topic/{topicName}/message
     */
    publish: (topicName: string, data: SendMessageRequest, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/topic/${topicName}/message`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        ...params,
      }),

    /**
     * @description subscribe a queue to a topic
     *
     * @tags topic
     * @name SubscribeQueue
     * @request POST:/api/v1/topic/{topicName}/subscribe
     */
    subscribeQueue: (topicName: string, data: SubscribeQueueRequest, params: RequestParams = {}) =>
      this.request<SubscribeResponse, void>({
        path: `/api/v1/topic/${topicName}/subscribe`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description create a queue
     *
     * @tags queue
     * @name CreateQueue
     * @request POST:/api/v1/queue
     */
    createQueue: (data: CreateQueueRequest, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/queue`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        ...params,
      }),

    /**
     * @description gets a list of all queues
     *
     * @tags queue
     * @name GetQueues
     * @request GET:/api/v1/queue
     */
    getQueues: (params: RequestParams = {}) =>
      this.request<GetQueuesResponse, any>({
        path: `/api/v1/queue`,
        method: "GET",
        format: "json",
        ...params,
      }),

    /**
     * @description get queue info
     *
     * @tags queue
     * @name GetQueue
     * @request GET:/api/v1/queue/{queueName}
     */
    getQueue: (queueName: string, params: RequestParams = {}) =>
      this.request<GetQueueResponse, void>({
        path: `/api/v1/queue/${queueName}`,
        method: "GET",
        format: "json",
        ...params,
      }),

    /**
     * @description delete a queue
     *
     * @tags queue
     * @name DeleteQueue
     * @request DELETE:/api/v1/queue/{queueName}
     */
    deleteQueue: (queueName: string, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}`,
        method: "DELETE",
        ...params,
      }),

    /**
     * @description send a message to a queue
     *
     * @tags queue
     * @name SendMessage
     * @request POST:/api/v1/queue/{queueName}/message
     */
    sendMessage: (queueName: string, data: SendMessageRequest, params: RequestParams = {}) =>
      this.request<SendMessageResponse, void>({
        path: `/api/v1/queue/${queueName}/message`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description pauses the queue, all new receive message calls will return 0 messages until the queue is resumed
     *
     * @tags queue
     * @name Pause
     * @request POST:/api/v1/queue/{queueName}/pause
     */
    pause: (queueName: string, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/pause`,
        method: "POST",
        ...params,
      }),

    /**
     * @description resumes the queue
     *
     * @tags queue
     * @name Resume
     * @request POST:/api/v1/queue/{queueName}/resume
     */
    resume: (queueName: string, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/resume`,
        method: "POST",
        ...params,
      }),

    /**
     * @description move all visible messages from this queue to a new queue
     *
     * @tags queue
     * @name MoveMessages
     * @request POST:/api/v1/queue/{sourceQueueName}/message/move
     */
    moveMessages: (
      sourceQueueName: string,
      query: {
        /**
         * the target queue to move messages to
         * @example "queue1"
         */
        targetQueueName: string;
      },
      params: RequestParams = {}
    ) =>
      this.request<MoveMessagesResponse, void>({
        path: `/api/v1/queue/${sourceQueueName}/message/move`,
        method: "POST",
        query: query,
        format: "json",
        ...params,
      }),

    /**
     * @description delete a message from a queue
     *
     * @tags queue
     * @name DeleteMessage
     * @request POST:/api/v1/queue/{queueName}/message/{messageId}
     */
    deleteMessage: (
      queueName: string,
      messageId: string,
      query?: {
        /** optional receipt handle */
        receiptHandle?: string;
      },
      params: RequestParams = {}
    ) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/message/${messageId}`,
        method: "POST",
        query: query,
        ...params,
      }),

    /**
     * @description update a message
     *
     * @tags queue
     * @name UpdateMessage
     * @request PUT:/api/v1/queue/{queueName}/message/{messageId}
     */
    updateMessage: (
      queueName: string,
      messageId: string,
      query: {
        /**
         * optional receipt handle
         * @example "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
         */
        receiptHandle: string;
      },
      data: UpdateMessageRequest,
      params: RequestParams = {}
    ) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/message/${messageId}`,
        method: "PUT",
        query: query,
        body: data,
        type: ContentType.Json,
        ...params,
      }),

    /**
     * @description get a message in a queue
     *
     * @tags queue
     * @name GetMessage
     * @request GET:/api/v1/queue/{queueName}/message/{messageId}
     */
    getMessage: (queueName: string, messageId: string, params: RequestParams = {}) =>
      this.request<GetMessageResponse, void>({
        path: `/api/v1/queue/${queueName}/message/${messageId}`,
        method: "GET",
        format: "json",
        ...params,
      }),

    /**
     * @description nak a message
     *
     * @tags queue
     * @name NakMessage
     * @request PUT:/api/v1/queue/{queueName}/message/{messageId}/nak
     */
    nakMessage: (
      queueName: string,
      messageId: string,
      query: {
        /**
         * optional receipt handle
         * @example "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
         */
        receiptHandle: string;
        /** optional reason for nak'ing the message */
        reason?: string;
      },
      params: RequestParams = {}
    ) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/message/${messageId}/nak`,
        method: "PUT",
        query: query,
        ...params,
      }),

    /**
     * @description purge a queue of all messages
     *
     * @tags queue
     * @name PurgeQueue
     * @request POST:/api/v1/queue/{queueName}/purge
     */
    purgeQueue: (queueName: string, params: RequestParams = {}) =>
      this.request<void, void>({
        path: `/api/v1/queue/${queueName}/purge`,
        method: "POST",
        ...params,
      }),

    /**
     * @description receive messages from the queue
     *
     * @tags queue
     * @name ReceiveMessages
     * @request POST:/api/v1/queue/{queueName}/receive
     */
    receiveMessages: (queueName: string, data: ReceiveMessagesRequest, params: RequestParams = {}) =>
      this.request<ReceiveMessagesResponse, void>({
        path: `/api/v1/queue/${queueName}/receive`,
        method: "POST",
        body: data,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description peek messages in a queue
     *
     * @tags queue
     * @name PeekMessages
     * @request GET:/api/v1/queue/{queueName}/peek
     */
    peekMessages: (
      queueName: string,
      query?: {
        /**
         * maximum number of message to peek
         * @format double
         */
        maxNumberOfMessages?: number;
        /** true, to include not visible (received messages) */
        includeNotVisible?: boolean;
        /** true, to include delayed messages */
        includeDelayed?: boolean;
      },
      params: RequestParams = {}
    ) =>
      this.request<PeekMessagesResponse, void>({
        path: `/api/v1/queue/${queueName}/peek`,
        method: "GET",
        query: query,
        format: "json",
        ...params,
      }),
  };
}
