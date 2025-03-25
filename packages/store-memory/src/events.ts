export interface NewQueueMessageEvent {
  type: "new-queue-message";
  queueName: string;
}

export interface ResumeEvent {
  type: "resume";
}
