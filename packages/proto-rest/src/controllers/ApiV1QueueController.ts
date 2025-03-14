import {
  createLogger,
  DeleteDeadLetterQueueError,
  InvalidUpdateError,
  MessageNotFoundError,
  QueueAlreadyExistsError,
  QueueNotFoundError,
  ReceiptHandleIsInvalidError,
  Store,
} from "@nexq/core";
import createError from "http-errors";
import { Body, Controller, Delete, Get, Path, Post, Put, Query, Response, Route, SuccessResponse } from "tsoa";
import { CreateQueueRequest } from "../dto/CreateQueueRequest.js";
import { CreateQueueResponse } from "../dto/CreateQueueResponse.js";
import { GetQueuesResponse, GetQueuesResponseQueue } from "../dto/GetQueuesResponse.js";
import { SendMessageRequest } from "../dto/SendMessageRequest.js";
import { UpdateMessageRequest } from "../dto/UpdateMessageRequest.js";
import { DurationParseError, parseOptionalBytesSize, parseOptionalDurationIntoMs } from "../utils.js";
import { ReceiveMessagesRequest } from "../dto/ReceiveMessagesRequest.js";
import { ReceiveMessagesResponse, ReceiveMessagesResponseMessage } from "../dto/ReceiveMessagesResponse.js";

const logger = createLogger("Rest:ApiV1QueueController");

export interface User {
  userId: number;
  name?: string;
}

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
  @SuccessResponse("200", "Queue Created")
  @Response<void>(400, "bad request")
  @Response<void>(409, "queue already exists with different parameters")
  @Response<void>(404, "dead letter queue not found")
  public async createQueue(@Body() request: CreateQueueRequest): Promise<CreateQueueResponse> {
    try {
      const id = await this.store.createQueue(request.name, {
        delayMs: parseOptionalDurationIntoMs(request.delay),
        expiresMs: parseOptionalDurationIntoMs(request.expires),
        maxMessageSize: parseOptionalBytesSize(request.maximumMessageSize),
        messageRetentionPeriodMs: parseOptionalDurationIntoMs(request.messageRetentionPeriod),
        receiveMessageWaitTimeMs: parseOptionalDurationIntoMs(request.receiveMessageWaitTime),
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
        deadLetterQueueName: request.deadLetter?.queueName,
        maxReceiveCount: request.deadLetter?.maxReceiveCount,
      });
      return { id };
    } catch (err) {
      if (err instanceof DurationParseError) {
        throw createError.BadRequest(err.message);
      }
      if (err instanceof QueueAlreadyExistsError) {
        throw createError.Conflict("queue already exists");
      }
      if (err instanceof QueueNotFoundError && err.queueName === request.deadLetter?.queueName) {
        throw createError.NotFound(`dead letter queue not found`);
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
        queues: queues.map((q) => {
          return {
            name: q.name,
            numberOfMessage: q.numberOfMessages,
            numberOfMessagesNotVisible: q.numberOfMessagesNotVisible,
            numberOfMessagesDelayed: q.numberOfMessagesDelayed,
            created: q.created,
            lastModified: q.lastModified,
            delayMs: q.delayMs,
            expiresMs: q.expiresMs,
            expiresAt: q.expiresAt,
            maxMessageSize: q.maxMessageSize,
            messageRetentionPeriodMs: q.messageRetentionPeriodMs,
            receiveMessageWaitTimeMs: q.receiveMessageWaitTimeMs,
            visibilityTimeoutMs: q.visibilityTimeoutMs,
            tags: q.tags,
            deadLetter: q.deadLetter
              ? {
                  queueName: q.deadLetter.queueName,
                  maxReceiveCount: q.deadLetter.maxReceiveCount,
                }
              : undefined,
          } satisfies GetQueuesResponseQueue;
        }),
      };
    } catch (err) {
      logger.error(`failed to get queues`, err);
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
        throw createError.NotFound("queue not found");
      }
      if (err instanceof DeleteDeadLetterQueueError) {
        throw createError.BadRequest("cannot delete dead letter queue associated with a queue");
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
  @Response<void>(404, "queue not found")
  public async sendMessage(@Path() queueName: string, @Body() request: SendMessageRequest): Promise<void> {
    try {
      await this.store.sendMessage(queueName, request.body, {
        attributes: request.attributes,
        delayMs: parseOptionalDurationIntoMs(request.delay),
        priority: request.priority,
      });
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createError.NotFound("queue not found");
      }
      logger.error(`failed to send message`, err);
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
  @Post("{queueName}/message/{messageId}")
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
        throw createError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createError.NotFound("message found but receipt handle does not match");
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
        throw createError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createError.NotFound("message found but receipt handle does not match");
      }
      if (err instanceof InvalidUpdateError) {
        throw createError.BadRequest(err.message);
      }
      logger.error(`failed to update message`, err);
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
  @Put("{queueName}/message/{messageId}/nak")
  @SuccessResponse("200", "Message deleted")
  @Response<void>(404, "queue, message id, or receipt handle not found")
  @Response<void>(400, "visibility timeout update without receipt handle")
  public async nakMessage(
    @Path() queueName: string,
    @Path() messageId: string,
    @Query() receiptHandle: string
  ): Promise<void> {
    try {
      await this.store.nakMessage(queueName, messageId, receiptHandle);
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createError.NotFound("queue not found");
      }
      if (err instanceof MessageNotFoundError) {
        throw createError.NotFound("message id not found");
      }
      if (err instanceof ReceiptHandleIsInvalidError) {
        throw createError.NotFound("message found but receipt handle does not match");
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
        throw createError.NotFound("queue not found");
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
    @Body() request: ReceiveMessagesRequest
  ): Promise<ReceiveMessagesResponse> {
    try {
      const messages = await this.store.receiveMessages(queueName, {
        maxNumberOfMessages: request.maxNumberOfMessages,
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
        waitTimeMs: parseOptionalDurationIntoMs(request.waitTime),
      });
      return {
        messages: messages.map((m) => {
          return {
            id: m.id,
            receiptHandle: m.receiptHandle,
            priority: m.priority,
            attributes: m.attributes,
            sentTime: m.sentTime.toISOString(),
            bodyBase64: m.body.toString("base64"),
          } satisfies ReceiveMessagesResponseMessage;
        }),
      };
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createError.NotFound("queue not found");
      }
      logger.error(`failed to receive message`, err);
      throw err;
    }
  }
}
