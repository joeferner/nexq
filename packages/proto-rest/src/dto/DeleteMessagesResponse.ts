import { DeleteMessagesResultMessage } from "@nexq/core/build/dto/DeleteMessagesResult.js";

export interface DeleteMessagesResponse {
  messages: Record<string, DeleteMessagesResultMessage>;
}
