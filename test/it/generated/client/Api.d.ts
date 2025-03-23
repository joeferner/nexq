/// <reference types="node" resolution-mode="require"/>
/** Construct a type with a set of properties K of type T */
export declare type RecordStringString = Record<string, string>;
export interface CreateTopicRequest {
    /**
     * The name of the topic
     * @example "my-topic"
     */
    name: string;
    /** tags to apply to this topic */
    tags?: RecordStringString;
}
export declare enum TopicProtocolQueue {
    Queue = "Queue"
}
export interface TopicInfoQueueSubscription {
    id: string;
    protocol: TopicProtocolQueue;
    queueName: string;
}
export declare type TopicInfoSubscription = TopicInfoQueueSubscription;
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
export declare type NakExpireBehaviorOptions = DecreasePriorityByNakExpireBehavior | "retry" | "moveToEnd" | ((DecreasePriorityByNakExpireBehavior & "retry") | "moveToEnd");
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
import { AxiosInstance, AxiosRequestConfig, AxiosResponse, ResponseType } from "axios";
export declare type QueryParamsType = Record<string | number, any>;
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
export declare type RequestParams = Omit<FullRequestParams, "body" | "method" | "query" | "path">;
export interface ApiConfig<SecurityDataType = unknown> extends Omit<AxiosRequestConfig, "data" | "cancelToken"> {
    securityWorker?: (securityData: SecurityDataType | null) => Promise<AxiosRequestConfig | void> | AxiosRequestConfig | void;
    secure?: boolean;
    format?: ResponseType;
}
export declare enum ContentType {
    Json = "application/json",
    FormData = "multipart/form-data",
    UrlEncoded = "application/x-www-form-urlencoded",
    Text = "text/plain"
}
export declare class HttpClient<SecurityDataType = unknown> {
    instance: AxiosInstance;
    private securityData;
    private securityWorker?;
    private secure?;
    private format?;
    constructor({ securityWorker, secure, format, ...axiosConfig }?: ApiConfig<SecurityDataType>);
    setSecurityData: (data: SecurityDataType | null) => void;
    protected mergeRequestParams(params1: AxiosRequestConfig, params2?: AxiosRequestConfig): AxiosRequestConfig;
    protected stringifyFormItem(formItem: unknown): string;
    protected createFormData(input: Record<string, unknown>): FormData;
    request: <T = any, _E = any>({ secure, path, type, query, format, body, ...params }: FullRequestParams) => Promise<AxiosResponse<T, any>>;
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
export declare class NexqApi<SecurityDataType extends unknown> extends HttpClient<SecurityDataType> {
    api: {
        /**
         * @description create a topic
         *
         * @tags topic
         * @name CreateTopic
         * @request POST:/api/v1/topic
         */
        createTopic: (data: CreateTopicRequest, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description gets a list of all topics
         *
         * @tags topic
         * @name GetTopics
         * @request GET:/api/v1/topic
         */
        getTopics: (params?: RequestParams) => Promise<AxiosResponse<GetTopicsResponse, any>>;
        /**
         * @description get topic info
         *
         * @tags topic
         * @name GetTopic
         * @request GET:/api/v1/topic/{topicName}
         */
        getTopic: (topicName: string, params?: RequestParams) => Promise<AxiosResponse<GetTopicResponse, any>>;
        /**
         * @description delete a topic
         *
         * @tags topic
         * @name DeleteTopic
         * @request DELETE:/api/v1/topic/{topicName}
         */
        deleteTopic: (topicName: string, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description publish a message to a topic
         *
         * @tags topic
         * @name Publish
         * @request POST:/api/v1/topic/{topicName}/message
         */
        publish: (topicName: string, data: SendMessageRequest, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description subscribe a queue to a topic
         *
         * @tags topic
         * @name SubscribeQueue
         * @request POST:/api/v1/topic/{topicName}/subscribe
         */
        subscribeQueue: (topicName: string, data: SubscribeQueueRequest, params?: RequestParams) => Promise<AxiosResponse<SubscribeResponse, any>>;
        /**
         * @description create a queue
         *
         * @tags queue
         * @name CreateQueue
         * @request POST:/api/v1/queue
         */
        createQueue: (data: CreateQueueRequest, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description gets a list of all queues
         *
         * @tags queue
         * @name GetQueues
         * @request GET:/api/v1/queue
         */
        getQueues: (params?: RequestParams) => Promise<AxiosResponse<GetQueuesResponse, any>>;
        /**
         * @description get queue info
         *
         * @tags queue
         * @name GetQueue
         * @request GET:/api/v1/queue/{queueName}
         */
        getQueue: (queueName: string, params?: RequestParams) => Promise<AxiosResponse<GetQueueResponse, any>>;
        /**
         * @description delete a queue
         *
         * @tags queue
         * @name DeleteQueue
         * @request DELETE:/api/v1/queue/{queueName}
         */
        deleteQueue: (queueName: string, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description send a message to a queue
         *
         * @tags queue
         * @name SendMessage
         * @request POST:/api/v1/queue/{queueName}/message
         */
        sendMessage: (queueName: string, data: SendMessageRequest, params?: RequestParams) => Promise<AxiosResponse<SendMessageResponse, any>>;
        /**
         * @description move all visible messages from this queue to a new queue
         *
         * @tags queue
         * @name MoveMessages
         * @request POST:/api/v1/queue/{sourceQueueName}/message/move
         */
        moveMessages: (sourceQueueName: string, query: {
            /**
             * the target queue to move messages to
             * @example "queue1"
             */
            targetQueueName: string;
        }, params?: RequestParams) => Promise<AxiosResponse<MoveMessagesResponse, any>>;
        /**
         * @description delete a message from a queue
         *
         * @tags queue
         * @name DeleteMessage
         * @request POST:/api/v1/queue/{queueName}/message/{messageId}
         */
        deleteMessage: (queueName: string, messageId: string, query?: {
            /** optional receipt handle */
            receiptHandle?: string;
        }, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description update a message
         *
         * @tags queue
         * @name UpdateMessage
         * @request PUT:/api/v1/queue/{queueName}/message/{messageId}
         */
        updateMessage: (queueName: string, messageId: string, query: {
            /**
             * optional receipt handle
             * @example "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
             */
            receiptHandle: string;
        }, data: UpdateMessageRequest, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description get a message in a queue
         *
         * @tags queue
         * @name GetMessage
         * @request GET:/api/v1/queue/{queueName}/message/{messageId}
         */
        getMessage: (queueName: string, messageId: string, params?: RequestParams) => Promise<AxiosResponse<GetMessageResponse, any>>;
        /**
         * @description nak a message
         *
         * @tags queue
         * @name NakMessage
         * @request PUT:/api/v1/queue/{queueName}/message/{messageId}/nak
         */
        nakMessage: (queueName: string, messageId: string, query: {
            /**
             * optional receipt handle
             * @example "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
             */
            receiptHandle: string;
            /** optional reason for nak'ing the message */
            reason?: string;
        }, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description purge a queue of all messages
         *
         * @tags queue
         * @name PurgeQueue
         * @request POST:/api/v1/queue/{queueName}/purge
         */
        purgeQueue: (queueName: string, params?: RequestParams) => Promise<AxiosResponse<void, any>>;
        /**
         * @description receive messages from the queue
         *
         * @tags queue
         * @name ReceiveMessages
         * @request POST:/api/v1/queue/{queueName}/receive
         */
        receiveMessages: (queueName: string, data: ReceiveMessagesRequest, params?: RequestParams) => Promise<AxiosResponse<ReceiveMessagesResponse, any>>;
        /**
         * @description peek messages in a queue
         *
         * @tags queue
         * @name PeekMessages
         * @request GET:/api/v1/queue/{queueName}/peek
         */
        peekMessages: (queueName: string, query?: {
            /**
             * maximum number of message to peek
             * @format double
             */
            maxNumberOfMessages?: number;
            /** true, to include not visible (received messages) */
            includeNotVisible?: boolean;
            /** true, to include delayed messages */
            includeDelayed?: boolean;
        }, params?: RequestParams) => Promise<AxiosResponse<PeekMessagesResponse, any>>;
    };
}
//# sourceMappingURL=Api.d.ts.map