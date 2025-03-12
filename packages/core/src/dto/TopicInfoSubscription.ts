export enum TopicProtocol {
  Queue,
}

export interface TopicInfoQueueSubscription {
  id: string;
  protocol: TopicProtocol.Queue;
  queueName: string;
}

export type TopicInfoSubscription = TopicInfoQueueSubscription;
