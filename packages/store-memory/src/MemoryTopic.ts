import {
  createId,
  CreateTopicOptions,
  SendMessageOptions,
  SubscriptionNotFoundError,
  TopicInfo,
  TopicInfoQueueSubscription,
  TopicProtocol,
} from "@nexq/core";
import { MemoryQueue } from "./MemoryQueue.js";

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
      tags: structuredClone(this.tags),
      subscriptions: this.queueSubscriptions.map((s) => {
        return {
          protocol: TopicProtocol.Queue,
          id: s.id,
          queueName: s.queueName,
        } satisfies TopicInfoQueueSubscription;
      }),
    };
  }

  public subscribeQueue(queueName: string): string {
    const existing = this.queueSubscriptions.find((s) => s.queueName === queueName);
    if (existing) {
      return existing.id;
    }
    const id = createId();
    this.queueSubscriptions.push({ id, queueName });
    return id;
  }

  public publishMessage(
    body: string,
    getQueueRequired: (queueName: string) => MemoryQueue,
    options?: SendMessageOptions & { lastNakReason?: string }
  ): void {
    const queues = this.queueSubscriptions.map((s) => getQueueRequired(s.queueName));
    for (const queue of queues) {
      const id = createId();
      queue.sendMessage(id, body, options);
    }
  }

  public deleteSubscription(subscriptionId: string): void {
    const index = this.queueSubscriptions.findIndex((s) => s.id === subscriptionId);
    if (index < 0) {
      throw new SubscriptionNotFoundError(subscriptionId);
    }
    this.queueSubscriptions.splice(index, 1);
  }
}
