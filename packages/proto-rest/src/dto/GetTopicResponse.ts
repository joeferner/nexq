import { TopicInfo, TopicInfoSubscription } from "@nexq/core";

export interface GetTopicResponse {
  name: string;
  tags: Record<string, string>;
  subscriptions: TopicInfoSubscription[];
}

export function topicInfoToGetTopicResponse(t: TopicInfo): GetTopicResponse {
  return {
    name: t.name,
    tags: t.tags,
    subscriptions: t.subscriptions,
  } satisfies GetTopicResponse;
}
