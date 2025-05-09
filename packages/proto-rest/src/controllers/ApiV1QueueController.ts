import {
  AbortError,
  createLogger,
  DeleteDeadLetterQueueError,
  DurationParseError,
  InvalidUpdateError,
  MessageNotFoundError,
  parseOptionalBytesSize,
  parseOptionalDurationIntoMs,
  QueueAlreadyExistsError,
  QueueNotFoundError,
  ReceiptHandleIsInvalidError,
  Store,
  TopicNotFoundError,
} from "@nexq/core";
import { SendMessagesOptionsMessage } from "@nexq/core/build/dto/SendMessagesOptions.js";
import express from "express";
import createHttpError from "http-errors";
import {
  Body,
  Controller,
  Delete,
  Get,
  Path,
  Post,
  Put,
  Query,
  Request,
  Response,
  Route,
  SuccessResponse,
  Tags,
} from "tsoa";
import { CreateQueueRequest } from "../dto/CreateQueueRequest.js";
import { GetMessageResponse } from "../dto/GetMessageResponse.js";
import { GetQueueResponse, queueInfoToGetQueueResponse } from "../dto/GetQueueResponse.js";
import { GetQueuesResponse } from "../dto/GetQueuesResponse.js";
import { MoveMessagesResponse } from "../dto/MoveMessagesResponse.js";
import { PeekMessagesResponse, PeekMessagesResponseMessage } from "../dto/PeekMessagesResponse.js";
import { ReceiveMessagesRequest } from "../dto/ReceiveMessagesRequest.js";
import { ReceiveMessagesResponse, ReceiveMessagesResponseMessage } from "../dto/ReceiveMessagesResponse.js";
import { SendMessageRequest } from "../dto/SendMessageRequest.js";
import { SendMessageResponse } from "../dto/SendMessageResponse.js";
import { SendMessagesRequest } from "../dto/SendMessagesRequest.js";
import { SendMessagesResponse } from "../dto/SendMessagesResponse.js";
import { UpdateMessageRequest } from "../dto/UpdateMessageRequest.js";
import { isHttpError } from "../utils.js";

const logger = createLogger("Rest:ApiV1QueueController");

export interface User {
  userId: number;
  name?: string;
}

@Tags("queue")
@Route("api/v1/queue")
export class ApiV1QueueController extends Controller {
  public constructor(private readonly store: Store) {
    super();
  }

  /**
   * create a queue
   *
   * @param request the options for the new queue
   */
  @Post("")
  @SuccessResponse("204", "Queue Created")
  @Response<void>(400, "bad request")
  @Response<void>(409, "queue already exists with different parameters")
  @Response<void>(404, "dead letter queue/topic not found")
  public async createQueue(@Body() request: CreateQueueRequest): Promise<void> {
    try {
      await this.store.createQueue(request.name, {
        upsert: request.upsert ?? false,
        delayMs: parseOptionalDurationIntoMs(request.delay),
        expiresMs: parseOptionalDurationIntoMs(request.expires),
        maxMessageSize: parseOptionalBytesSize(request.maximumMessageSize),
        messageRetentionPeriodMs: parseOptionalDurationIntoMs(request.messageRetentionPeriod),
        receiveMessageWaitTimeMs: parseOptionalDurationIntoMs(request.receiveMessageWaitTime),
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
        deadLetterQueueName: request.deadLetterQueueName,
        deadLetterTopicName: request.deadLetterTopicName,
        maxReceiveCount: request.maxReceiveCount,
        nakExpireBehavior: request.nakExpireBehavior,
        tags: request.tags,
      });
    } catch (err) {
      if (err instanceof DurationParseError) {
        throw createHttpError.BadRequest(err.message);
      }
      if (err instanceof QueueAlreadyExistsError) {
        throw createHttpError.Conflict(`queue already exists: ${err.reason}`);
      }
      if (err instanceof QueueNotFoundError && err.queueName === request.deadLetterQueueName) {
        throw createHttpError.NotFound(`dead letter queue not found`);
      }
      if (err instanceof TopicNotFoundError && err.topicName === request.deadLetterTopicName) {
        throw createHttpError.NotFound(`dead letter topic not found`);
      }
      logger.error(`failed to create queue`, err);
      throw err;
    }
  }

  /**
   * gets a list of all queues
   */
  @Get("")
  @SuccessResponse("200", "List of queues")
  public async getQueues(): Promise<GetQueuesResponse> {
    try {
      const queues = await this.store.getQueueInfos();
      return {
        queues: queues.map(queueInfoToGetQueueResponse),
      };
    } catch (err) {
      logger.error(`failed to get queues`, err);
      throw err;
    }
  }

  /**
   * get queue info
   *
   * @param queueName the name of the queue to get info on
   * @example queueName "queue1"
   */
  @Get("{queueName}")
  @SuccessResponse("200", "Queue info")
  @Response<void>(404, "queue not found")
  public async getQueue(@Path() queueName: string): Promise<GetQueueResponse> {
    try {
      return queueInfoToGetQueueResponse(await this.store.getQueueInfo(queueName));
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to get queue info`, err);
      throw err;
    }
  }

  /**
   * delete a queue
   *
   * @param queueName the name of the queue to delete
   * @example queueName "queue1"
   */
  @Delete("{queueName}")
  @SuccessResponse("200", "Queue deleted")
  @Response<void>(400, "cannot delete dead letter queue associated with a queue")
  @Response<void>(404, "queue not found")
  public async deleteQueue(@Path() queueName: string): Promise<void> {
    try {
      await this.store.deleteQueue(queueName);
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof DeleteDeadLetterQueueError) {
        throw createHttpError.BadRequest("cannot delete dead letter queue associated with a queue");
      }
      logger.error(`failed to delete queue`, err);
      throw err;
    }
  }

  /**
   * send a message to a queue
   *
   * @param queueName the name of the queue to send to
   * @param request the options for the message
   * @example queueName "queue1"
   */
  @Post("{queueName}/message")
  @SuccessResponse("200", "Message sent")
  @Response<void>(400, "invalid body")
  @Response<void>(404, "queue not found")
  public async sendMessage(
    @Path() queueName: string,
    @Body() request: SendMessageRequest
  ): Promise<SendMessageResponse> {
    try {
      const m = await this.store.sendMessage(queueName, request.body, {
        attributes: request.attributes,
        delayMs: parseOptionalDurationIntoMs(request.delay),
        priority: request.priority,
      });
      return { id: m.id };
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to send message`, err);
      throw err;
    }
  }

  /**
   * send messages to a queue
   *
   * @param queueName the name of the queue to send to
   * @param request the options for the messages
   * @example queueName "queue1"
   */
  @Post("{queueName}/messages")
  @SuccessResponse("200", "Messages sent")
  @Response<void>(400, "invalid body")
  @Response<void>(404, "queue not found")
  public async sendMessages(
    @Path() queueName: string,
    @Body() request: SendMessagesRequest
  ): Promise<SendMessagesResponse> {
    try {
      const messages: SendMessagesOptionsMessage[] = request.messages.map((message) => {
        return {
          body: message.body,
          attributes: message.attributes,
          delayMs: parseOptionalDurationIntoMs(message.delay),
          priority: message.priority,
        } satisfies SendMessagesOptionsMessage;
      });
      const m = await this.store.sendMessages(queueName, { messages });
      return { ids: m.ids };
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to send messages`, err);
      throw err;
    }
  }

  /**
   * pauses the queue, all new receive message calls will return 0 messages until the queue is resumed
   *
   * @param queueName the name of the queue to pause
   * @example queueName "queue1"
   */
  @Post("{queueName}/pause")
  @SuccessResponse("200", "Queue paused")
  @Response<void>(404, "queue not found")
  public async pauseQueue(@Path() queueName: string): Promise<void> {
    try {
      await this.store.pause(queueName);
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to pause queue`, err);
      throw err;
    }
  }

  /**
   * resumes the queue
   *
   * @param queueName the name of the queue to resume
   * @example queueName "queue1"
   */
  @Post("{queueName}/resume")
  @SuccessResponse("200", "Queue resumed")
  @Response<void>(404, "queue not found")
  public async resumeQueue(@Path() queueName: string): Promise<void> {
    try {
      await this.store.resume(queueName);
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to resume queue`, err);
      throw err;
    }
  }

  /**
   * move all visible messages from this queue to a new queue
   *
   * @param sourceQueueName the name of the queue to move messages from
   * @param targetQueueName the target queue to move messages to
   * @example sourceQueueName "queue1-dlq"
   * @example targetQueueName "queue1"
   */
  @Post("{sourceQueueName}/message/move")
  @SuccessResponse("200", "Messages moved")
  @Response<void>(404, "queue not found")
  public async moveMessages(
    @Path() sourceQueueName: string,
    @Query() targetQueueName: string
  ): Promise<MoveMessagesResponse> {
    try {
      const result = await this.store.moveMessages(sourceQueueName, targetQueueName);
      return { movedMessageCount: result.movedMessageCount };
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to move messages`, err);
      throw err;
    }
  }

  /**
   * delete a message from a queue
   *
   * @param queueName the name of the queue to send to
   * @param messageId the id of the message to delete
   * @param receiptHandle optional receipt handle
   * @example queueName "queue1"
   * @example messageId "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
   */
  @Delete("{queueName}/message/{messageId}")
  @SuccessResponse("200", "Message deleted")
  @Response<void>(404, "queue, message id, or receipt handle not found")
  public async deleteMessage(
    @Path() queueName: string,
    @Path() messageId: string,
    @Query() receiptHandle?: string
  ): Promise<void> {
    try {
      await this.store.deleteMessage(queueName, messageId, receiptHandle);
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createHttpError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createHttpError.NotFound("message found but receipt handle does not match");
      }
      logger.error(`failed to delete message`, err);
      throw err;
    }
  }

  /**
   * update a message
   *
   * @param queueName the name of the queue to send to
   * @param messageId the id of the message to delete
   * @param receiptHandle optional receipt handle
   * @example queueName "queue1"
   * @example messageId     "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
   * @example receiptHandle "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
   */
  @Put("{queueName}/message/{messageId}")
  @SuccessResponse("200", "Message deleted")
  @Response<void>(404, "queue, message id, or receipt handle not found")
  @Response<void>(400, "visibility timeout update without receipt handle")
  public async updateMessage(
    @Path() queueName: string,
    @Path() messageId: string,
    @Query() receiptHandle: string | undefined,
    @Body() request: UpdateMessageRequest
  ): Promise<void> {
    try {
      await this.store.updateMessage(queueName, messageId, receiptHandle, {
        priority: request.priority,
        attributes: request.attributes,
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
      });
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createHttpError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createHttpError.NotFound("message found but receipt handle does not match");
      }
      if (err instanceof InvalidUpdateError) {
        throw createHttpError.BadRequest(err.message);
      }
      logger.error(`failed to update message`, err);
      throw err;
    }
  }

  /**
   * nak a message
   *
   * @param queueName the name of the queue to send to
   * @param messageId the id of the message to delete
   * @param receiptHandle optional receipt handle
   * @param reason optional reason for nak'ing the message
   * @example queueName "queue1"
   * @example messageId     "1effd43f-efc0-64a0-abb1-a262ad6a08d6"
   * @example receiptHandle "f9c729c6-cb6d-4d1a-b3eb-9d0254eb2e27"
   */
  @Put("{queueName}/message/{messageId}/nak")
  @SuccessResponse("200", "Message deleted")
  @Response<void>(404, "queue, message id, or receipt handle not found")
  @Response<void>(400, "visibility timeout update without receipt handle")
  public async nakMessage(
    @Path() queueName: string,
    @Path() messageId: string,
    @Query() receiptHandle: string,
    @Query() reason?: string
  ): Promise<void> {
    try {
      await this.store.nakMessage(queueName, messageId, receiptHandle, reason);
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createHttpError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createHttpError.NotFound("message found but receipt handle does not match");
      }
      logger.error(`failed to nak message`, err);
      throw err;
    }
  }

  /**
   * purge a queue of all messages
   *
   * @param queueName the name of the queue to purge
   * @example queueName "queue1"
   */
  @Post("{queueName}/purge")
  @SuccessResponse("200", "Queue purged")
  @Response<void>(404, "queue not found")
  public async purgeQueue(@Path() queueName: string): Promise<void> {
    try {
      await this.store.purgeQueue(queueName);
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to purge queue`, err);
      throw err;
    }
  }

  /**
   * receive messages from the queue
   *
   * @param queueName the name of the queue to receive messages from
   * @example queueName "queue1"
   */
  @Post("{queueName}/receive")
  @SuccessResponse("200", "messages received")
  @Response<void>(404, "queue not found")
  public async receiveMessages(
    @Path() queueName: string,
    @Body() request: ReceiveMessagesRequest,
    @Request() expressRequest: express.Request
  ): Promise<ReceiveMessagesResponse> {
    try {
      const abortController = new AbortController();
      expressRequest.socket.on("close", () => {
        abortController.abort();
      });

      const messages = await this.store.receiveMessages(queueName, {
        maxNumberOfMessages: request.maxNumberOfMessages,
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
        waitTimeMs: parseOptionalDurationIntoMs(request.waitTime),
        abortSignal: abortController.signal,
      });
      return {
        messages: messages.map((m) => {
          return {
            id: m.id,
            receiptHandle: m.receiptHandle,
            priority: m.priority,
            attributes: m.attributes,
            sentTime: m.sentTime.toISOString(),
            body: m.body,
          } satisfies ReceiveMessagesResponseMessage;
        }),
      };
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof AbortError) {
        logger.debug("receive aborted");
        throw createHttpError.RequestTimeout();
      }
      logger.error(`failed to receive messages`, err);
      throw err;
    }
  }

  /**
   * peek messages in a queue
   *
   * @param queueName the name of the queue to peek messages from
   * @param maxNumberOfMessages maximum number of message to peek
   * @param includeDelayed true, to include delayed messages
   * @param includeNotVisible true, to include not visible (received messages)
   * @isInt maxNumberOfMessages
   * @example queueName "queue1"
   */
  @Get("{queueName}/peek")
  @SuccessResponse("200", "messages peeked")
  @Response<void>(404, "queue not found")
  public async peekMessages(
    @Path() queueName: string,
    @Query("maxNumberOfMessages") maxNumberOfMessages?: number,
    @Query("includeNotVisible") includeNotVisible?: boolean,
    @Query("includeDelayed") includeDelayed?: boolean
  ): Promise<PeekMessagesResponse> {
    try {
      const messages = await this.store.peekMessages(queueName, {
        maxNumberOfMessages,
        includeDelayed,
        includeNotVisible,
      });
      return {
        messages: messages.map((m) => {
          return {
            id: m.id,
            priority: m.priority,
            attributes: m.attributes,
            sentTime: m.sentTime.toISOString(),
            body: m.body,
            lastNakReason: m.lastNakReason,
            delayUntil: m.delayUntil?.toDateString(),
            isAvailable: m.isAvailable,
            receiveCount: m.receiveCount,
            expiresAt: m.expiresAt?.toISOString(),
            receiptHandle: m.receiptHandle,
            firstReceivedAt: m.firstReceivedAt?.toISOString(),
          } satisfies PeekMessagesResponseMessage;
        }),
      };
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to peek messages`, err);
      throw err;
    }
  }

  /**
   * get a message in a queue
   *
   * @param queueName the name of the queue to get a message from
   * @param messageId the id of the message to get
   * @example queueName "queue1"
   */
  @Get("{queueName}/message/{messageId}")
  @SuccessResponse("200", "got message")
  @Response<void>(404, "queue or message not found")
  public async getMessage(@Path() queueName: string, @Path() messageId: string): Promise<GetMessageResponse> {
    try {
      const message = await this.store.getMessage(queueName, messageId);
      return {
        id: message.id,
        priority: message.priority,
        attributes: message.attributes,
        sentTime: message.sentTime.toISOString(),
        body: message.body,
        positionInQueue: message.positionInQueue,
        delayUntil: message.delayUntil?.toISOString(),
        isAvailable: message.isAvailable,
        receiveCount: message.receiveCount,
        expiresAt: message.expiresAt?.toISOString(),
        receiptHandle: message.receiptHandle,
        firstReceivedAt: message.firstReceivedAt?.toISOString(),
        lastNakReason: message.lastNakReason,
      } satisfies GetMessageResponse;
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createHttpError.NotFound("message not found");
      }
      logger.error(`failed to get message`, err);
      throw err;
    }
  }
}
