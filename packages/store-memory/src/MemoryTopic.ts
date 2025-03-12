import {
  createId,
  CreateTopicOptions,
  QueueAlreadySubscribedToTopicError,
  TopicInfo,
  TopicInfoQueueSubscription,
  TopicProtocol,
} from "@nexq/core";
import * as R from "remeda";

interface QueueSubscription {
  id: string;
  queueName: string;
}

export class MemoryTopic {
  public readonly name: string;
  private readonly tags: Record<string, string>;
  public readonly queueSubscriptions: QueueSubscription[] = [];

  public constructor(name: string, options: CreateTopicOptions) {
    this.name = name;
    this.tags = options.tags ?? {};
  }

  public getInfo(): TopicInfo {
    return {
      name: this.name,
      subscriptions: this.queueSubscriptions.map((s) => {
        return {
          protocol: TopicProtocol.Queue,
          id: s.id,
          queueName: s.queueName,
        } satisfies TopicInfoQueueSubscription;
      }),
    };
  }

  public equalCreateTopicOptions(options: CreateTopicOptions): boolean {
    return R.isDeepEqual(this.tags, options.tags);
  }

  public subscribeQueue(queueName: string): string {
    if (this.queueSubscriptions.some((s) => s.queueName === queueName)) {
      throw new QueueAlreadySubscribedToTopicError(this.name, queueName);
    }
    const id = createId();
    this.queueSubscriptions.push({ id, queueName });
    return id;
  }
}
