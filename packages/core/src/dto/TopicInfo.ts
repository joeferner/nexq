import * as R from "radash";
import { CreateTopicOptions } from "./CreateTopicOptions.js";
import { TopicInfoSubscription } from "./TopicInfoSubscription.js";

export interface TopicInfo {
  name: string;
  subscriptions: TopicInfoSubscription[];
  tags: Record<string, string>;
}

export function topicInfoEqualCreateTopicOptions(topicInfo: TopicInfo, options: CreateTopicOptions): boolean {
  return R.isEqual(topicInfo.tags, options.tags);
}
