import { Message } from "../formatter/Formatter.js";
import { toInteger } from "../utils.js";
import { TransportJsonConfigObj } from "./config.js";
import { toTransportOptions, Transport, TransportOptions } from "./Transport.js";

export interface MemoryTransportOptions extends TransportOptions {
  maxMessages?: number;
}

function toMemoryTransportOptions(options?: MemoryTransportOptions | TransportJsonConfigObj): MemoryTransportOptions {
  if (!options) {
    return {};
  }

  let maxMessages: number | undefined;
  if ("maxMessages" in options && options.maxMessages !== undefined) {
    try {
      maxMessages = toInteger(options.maxMessages);
    } catch (err) {
      throw new Error('"maxMessages" is invalid', { cause: err });
    }
  } else {
    maxMessages = undefined;
  }

  return {
    ...toTransportOptions(options),
    maxMessages,
  };
}

export class MemoryTransport implements Transport {
  private maxMessages: number | undefined;
  public messages: Message[] = [];

  public constructor(options?: MemoryTransportOptions | TransportJsonConfigObj) {
    const opts = toMemoryTransportOptions(options);
    this.maxMessages = opts.maxMessages;
  }

  public log(message: Message): void {
    this.messages.push(message);
    if (this.maxMessages !== undefined && this.messages.length > this.maxMessages) {
      this.messages.splice(0, this.messages.length - this.maxMessages);
    }
  }
}
