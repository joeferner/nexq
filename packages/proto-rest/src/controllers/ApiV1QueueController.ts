import {
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
} from "@nexq/core";
import createHttpError from "http-errors";
import { Body, Controller, Delete, Get, Path, Post, Put, Query, Response, Route, SuccessResponse, Tags } from "tsoa";
import { CreateQueueRequest } from "../dto/CreateQueueRequest.js";
import { GetQueueResponse, queueInfoToGetQueueResponse } from "../dto/GetQueueResponse.js";
import { GetQueuesResponse } from "../dto/GetQueuesResponse.js";
import { MoveMessagesResponse } from "../dto/MoveMessagesResponse.js";
import { ReceiveMessagesRequest } from "../dto/ReceiveMessagesRequest.js";
import { ReceiveMessagesResponse, ReceiveMessagesResponseMessage } from "../dto/ReceiveMessagesResponse.js";
import { SendMessageRequest } from "../dto/SendMessageRequest.js";
import { SendMessageResponse } from "../dto/SendMessageResponse.js";
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
  @Response<void>(404, "dead letter queue not found")
  public async createQueue(@Body() request: CreateQueueRequest): Promise<void> {
    try {
      await this.store.createQueue(request.name, {
        delayMs: parseOptionalDurationIntoMs(request.delay),
        expiresMs: parseOptionalDurationIntoMs(request.expires),
        maxMessageSize: parseOptionalBytesSize(request.maximumMessageSize),
        messageRetentionPeriodMs: parseOptionalDurationIntoMs(request.messageRetentionPeriod),
        receiveMessageWaitTimeMs: parseOptionalDurationIntoMs(request.receiveMessageWaitTime),
        visibilityTimeoutMs: parseOptionalDurationIntoMs(request.visibilityTimeout),
        deadLetterQueueName: request.deadLetter?.queueName,
        maxReceiveCount: request.deadLetter?.maxReceiveCount,
        tags: request.tags,
      });
    } catch (err) {
      if (err instanceof DurationParseError) {
        throw createHttpError.BadRequest(err.message);
      }
      if (err instanceof QueueAlreadyExistsError) {
        throw createHttpError.Conflict(`queue already exists: ${err.reason}`);
      }
      if (err instanceof QueueNotFoundError && err.queueName === request.deadLetter?.queueName) {
        throw createHttpError.NotFound(`dead letter queue not found`);
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
   * update a message
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
            body: m.body,
          } satisfies ReceiveMessagesResponseMessage;
        }),
      };
    } catch (err) {
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to receive message`, err);
      throw err;
    }
  }
}
