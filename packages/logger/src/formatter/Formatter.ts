import { LogLevel } from "../LogLevel.js";
import { MessageContext } from "../MessageContext.js";
import { FormatterJsonConfig } from "./config.js";

export type FormatterOptions = object;

export function toFormatterOptions(_options: FormatterJsonConfig | FormatterOptions): FormatterOptions {
  return {};
}

export interface Message {
  time: Date;
  level: LogLevel;
  loggerName?: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  message: any;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  params: any[];
  messageContext?: MessageContext;
}

export interface FormatterMessageOptions {
  enableColor: boolean;
}

export interface Formatter {
  formatMessage(message: Message, options: FormatterMessageOptions): string;
}
