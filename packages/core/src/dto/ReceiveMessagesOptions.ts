import { ReceiveMessageOptions } from "./ReceiveMessageOptions.js";

export interface ReceiveMessagesOptions extends ReceiveMessageOptions {
  maxNumberOfMessages?: number;
}
