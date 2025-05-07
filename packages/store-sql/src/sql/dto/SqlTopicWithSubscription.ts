import { TopicInfo, TopicInfoQueueSubscription, TopicProtocol } from "@nexq/core";
import { SqlTopic } from "./SqlTopic.js";

export interface SqlTopicWithSubscription extends SqlTopic {
  subscription_id: string;
  queue_name: string;
}

export function sqlTopicWithSubscriptionToTopicInfo(rows: SqlTopicWithSubscription[]): TopicInfo {
  return {
    name: rows[0].name,
    tags: JSON.parse(rows[0].tags) as Record<string, string>,
    subscriptions: rows
      .filter((row) => row.subscription_id !== null)
      .map((row) => {
        return {
          id: row.subscription_id,
          protocol: TopicProtocol.Queue,
          queueName: row.queue_name,
        } satisfies TopicInfoQueueSubscription;
      }),
  };
}
