import { SendMessageOptions } from "./SendMessageOptions.js";

export interface SendMessagesOptionsMessage extends SendMessageOptions {
  /**
   * Body of the message
   */
  body: string;
}

export interface SendMessagesOptions {
  messages: SendMessagesOptionsMessage[];
}
