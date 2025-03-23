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
export var TopicProtocolQueue;
(function (TopicProtocolQueue) {
    TopicProtocolQueue["Queue"] = "Queue";
})(TopicProtocolQueue = TopicProtocolQueue || (TopicProtocolQueue = {}));
import axios from "axios";
export var ContentType;
(function (ContentType) {
    ContentType["Json"] = "application/json";
    ContentType["FormData"] = "multipart/form-data";
    ContentType["UrlEncoded"] = "application/x-www-form-urlencoded";
    ContentType["Text"] = "text/plain";
})(ContentType = ContentType || (ContentType = {}));
export class HttpClient {
    instance;
    securityData = null;
    securityWorker;
    secure;
    format;
    constructor({ securityWorker, secure, format, ...axiosConfig } = {}) {
        this.instance = axios.create({ ...axiosConfig, baseURL: axiosConfig.baseURL || "/" });
        this.secure = secure;
        this.format = format;
        this.securityWorker = securityWorker;
    }
    setSecurityData = (data) => {
        this.securityData = data;
    };
    mergeRequestParams(params1, params2) {
        const method = params1.method || (params2 && params2.method);
        return {
            ...this.instance.defaults,
            ...params1,
            ...(params2 || {}),
            headers: {
                ...((method && this.instance.defaults.headers[method.toLowerCase()]) || {}),
                ...(params1.headers || {}),
                ...((params2 && params2.headers) || {}),
            },
        };
    }
    stringifyFormItem(formItem) {
        if (typeof formItem === "object" && formItem !== null) {
            return JSON.stringify(formItem);
        }
        else {
            return `${formItem}`;
        }
    }
    createFormData(input) {
        return Object.keys(input || {}).reduce((formData, key) => {
            const property = input[key];
            const propertyContent = property instanceof Array ? property : [property];
            for (const formItem of propertyContent) {
                const isFileType = formItem instanceof Blob || formItem instanceof File;
                formData.append(key, isFileType ? formItem : this.stringifyFormItem(formItem));
            }
            return formData;
        }, new FormData());
    }
    request = async ({ secure, path, type, query, format, body, ...params }) => {
        const secureParams = ((typeof secure === "boolean" ? secure : this.secure) &&
            this.securityWorker &&
            (await this.securityWorker(this.securityData))) ||
            {};
        const requestParams = this.mergeRequestParams(params, secureParams);
        const responseFormat = format || this.format || undefined;
        if (type === ContentType.FormData && body && body !== null && typeof body === "object") {
            body = this.createFormData(body);
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
export class NexqApi extends HttpClient {
    api = {
        /**
         * @description create a topic
         *
         * @tags topic
         * @name CreateTopic
         * @request POST:/api/v1/topic
         */
        createTopic: (data, params = {}) => this.request({
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
        getTopics: (params = {}) => this.request({
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
        getTopic: (topicName, params = {}) => this.request({
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
        deleteTopic: (topicName, params = {}) => this.request({
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
        publish: (topicName, data, params = {}) => this.request({
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
        subscribeQueue: (topicName, data, params = {}) => this.request({
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
        createQueue: (data, params = {}) => this.request({
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
        getQueues: (params = {}) => this.request({
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
        getQueue: (queueName, params = {}) => this.request({
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
        deleteQueue: (queueName, params = {}) => this.request({
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
        sendMessage: (queueName, data, params = {}) => this.request({
            path: `/api/v1/queue/${queueName}/message`,
            method: "POST",
            body: data,
            type: ContentType.Json,
            format: "json",
            ...params,
        }),
        /**
         * @description move all visible messages from this queue to a new queue
         *
         * @tags queue
         * @name MoveMessages
         * @request POST:/api/v1/queue/{sourceQueueName}/message/move
         */
        moveMessages: (sourceQueueName, query, params = {}) => this.request({
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
        deleteMessage: (queueName, messageId, query, params = {}) => this.request({
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
        updateMessage: (queueName, messageId, query, data, params = {}) => this.request({
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
        getMessage: (queueName, messageId, params = {}) => this.request({
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
        nakMessage: (queueName, messageId, query, params = {}) => this.request({
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
        purgeQueue: (queueName, params = {}) => this.request({
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
        receiveMessages: (queueName, data, params = {}) => this.request({
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
        peekMessages: (queueName, query, params = {}) => this.request({
            path: `/api/v1/queue/${queueName}/peek`,
            method: "GET",
            query: query,
            format: "json",
            ...params,
        }),
    };
}
//# sourceMappingURL=Api.js.map