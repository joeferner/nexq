export interface NewQueueMessageEvent {
  type: "newQueueMessageEvent";
  queueName: string;
}

export interface ResumeEvent {
  type: "resumeEvent";
  queueName: string;
}
