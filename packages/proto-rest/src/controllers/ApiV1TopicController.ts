import {
  createLogger,
  parseOptionalDurationIntoMs,
  QueueNotFoundError,
  Store,
  TopicAlreadyExistsError,
  TopicNotFoundError,
  TopicProtocol,
} from "@nexq/core";
import createHttpError from "http-errors";
import { Body, Controller, Delete, Get, Path, Post, Response, Route, SuccessResponse, Tags } from "tsoa";
import { CreateTopicRequest } from "../dto/CreateTopicRequest.js";
import { GetTopicResponse, topicInfoToGetTopicResponse } from "../dto/GetTopicResponse.js";
import { GetTopicsResponse } from "../dto/GetTopicsResponse.js";
import { SendMessageRequest } from "../dto/SendMessageRequest.js";
import { SendMessageResponse } from "../dto/SendMessageResponse.js";
import { SubscribeQueueRequest } from "../dto/SubscribeQueueRequest.js";
import { SubscribeResponse } from "../dto/SubscribeResponse.js";
import { isHttpError } from "../utils.js";

const logger = createLogger("Rest:ApiV1TopicController");

@Tags("topic")
@Route("api/v1/topic")
export class ApiV1TopicController extends Controller {
  public constructor(private readonly store: Store) {
    super();
  }

  /**
   * create a topic
   *
   * @param request the options for the new topic
   */
  @Post("")
  @SuccessResponse("204", "Topic Created")
  @Response<void>(409, "topic already exists with different parameters")
  public async createTopic(@Body() request: CreateTopicRequest): Promise<void> {
    try {
      await this.store.createTopic(request.name, {
        tags: request.tags,
      });
    } catch (err) {
      if (err instanceof TopicAlreadyExistsError) {
        throw createHttpError.Conflict(`topic already exists: ${err.reason}`);
      }
      logger.error(`failed to create topic`, err);
      throw err;
    }
  }

  /**
   * gets a list of all topics
   */
  @Get("")
  @SuccessResponse("200", "List of topics")
  public async getTopics(): Promise<GetTopicsResponse> {
    try {
      const topics = await this.store.getTopicInfos();
      return {
        topics: topics.map(topicInfoToGetTopicResponse),
      };
    } catch (err) {
      logger.error(`failed to get topics`, err);
      throw err;
    }
  }

  /**
   * get topic info
   *
   * @param topicName the name of the topic to get info on
   * @example topicName "topic1"
   */
  @Get("{topicName}")
  @SuccessResponse("200", "Topic info")
  @Response<void>(404, "topic not found")
  public async getTopic(@Path() topicName: string): Promise<GetTopicResponse> {
    try {
      return topicInfoToGetTopicResponse(await this.store.getTopicInfo(topicName));
    } catch (err) {
      if (err instanceof TopicNotFoundError) {
        throw createHttpError.NotFound("topic not found");
      }
      logger.error(`failed to get topic info`, err);
      throw err;
    }
  }

  /**
   * delete a topic
   *
   * @param topicName the name of the topic to delete
   * @example topicName "topic1"
   */
  @Delete("{topicName}")
  @SuccessResponse("200", "Topic deleted")
  @Response<void>(404, "topic not found")
  public async deleteTopic(@Path() topicName: string): Promise<void> {
    try {
      await this.store.deleteTopic(topicName);
    } catch (err) {
      if (err instanceof TopicNotFoundError) {
        throw createHttpError.NotFound("topic not found");
      }
      logger.error(`failed to delete topic`, err);
      throw err;
    }
  }

  /**
   * publish a message to a topic
   *
   * @param topicName the name of the topic to send to
   * @param request the options for the message
   * @example topicName "topic1"
   */
  @Post("{topicName}/message")
  @SuccessResponse("200", "Message sent")
  @Response<void>(400, "invalid body")
  @Response<void>(404, "topic not found")
  public async publish(@Path() topicName: string, @Body() request: SendMessageRequest): Promise<SendMessageResponse> {
    try {
      const m = await this.store.publishMessage(topicName, request.body, {
        attributes: request.attributes,
        delayMs: parseOptionalDurationIntoMs(request.delay),
        priority: request.priority,
      });
      return { id: m.id };
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof TopicNotFoundError) {
        throw createHttpError.NotFound("topic not found");
      }
      logger.error(`failed to publish`, err);
      throw err;
    }
  }

  /**
   * subscribe a queue to a topic
   *
   * @param topicName the name of the topic to subscribe to
   * @param request the options for the subscription
   * @example topicName "topic1"
   */
  @Post("{topicName}/subscribe")
  @SuccessResponse("200", "Topic subscribed")
  @Response<void>(404, "topic or queue not found")
  public async subscribeQueue(
    @Path() topicName: string,
    @Body() request: SubscribeQueueRequest
  ): Promise<SubscribeResponse> {
    try {
      const id = await this.store.subscribe(topicName, TopicProtocol.Queue, request.queueName);
      return { id };
    } catch (err) {
      if (isHttpError(err)) {
        throw err;
      }
      if (err instanceof TopicNotFoundError) {
        throw createHttpError.NotFound("topic not found");
      }
      if (err instanceof QueueNotFoundError) {
        throw createHttpError.NotFound("queue not found");
      }
      logger.error(`failed to subscribe`, err);
      throw err;
    }
  }
}
