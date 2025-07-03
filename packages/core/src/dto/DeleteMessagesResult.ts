export interface DeleteMessagesResult {
  messages: Record<string, DeleteMessagesResultMessage>;
}

export interface DeleteMessagesResultMessage {
  deleted: boolean;
  error?: DeleteMessagesResultError;
  errorMessage?: string;
}

export enum DeleteMessagesResultError {
  MessageNotFound = "MessageNotFound",
  ReceiptHandleIsInvalid = "ReceiptHandleIsInvalid",
}
