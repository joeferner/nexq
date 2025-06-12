import { Formatter, Message } from "../formatter/Formatter.js";
import { LogLevel } from "../LogLevel.js";
import { toBoolean } from "../utils.js";
import { TransportJsonConfigObj } from "./config.js";
import { toTransportOptions, Transport, TransportOptions } from "./Transport.js";

export interface ConsoleTransportOptions extends TransportOptions {
  enableColor?: boolean;
}

function toConsoleTransportOptions(
  options: ConsoleTransportOptions | TransportJsonConfigObj | undefined
): ConsoleTransportOptions {
  if (!options) {
    return {};
  }

  let enableColor: boolean | undefined;
  if (options.enableColor !== undefined) {
    try {
      enableColor = toBoolean(options.enableColor);
    } catch (err) {
      throw new Error('"enableColor" is invalid', { cause: err });
    }
  } else {
    enableColor = undefined;
  }

  return {
    ...toTransportOptions(options),
    enableColor,
  };
}

export class ConsoleTransport implements Transport {
  private readonly enableColor: boolean;

  public constructor(options?: ConsoleTransportOptions | TransportJsonConfigObj) {
    const opts = toConsoleTransportOptions(options);
    this.enableColor = opts.enableColor ?? true;
  }

  public log(message: Message, formatter: Formatter): void {
    const formattedMessage = formatter.formatMessage(message, { enableColor: this.enableColor });
    if (message.level <= LogLevel.Error) {
      // eslint-disable-next-line no-console
      console.error(formattedMessage);
    } else if (message.level <= LogLevel.Warn) {
      // eslint-disable-next-line no-console
      console.warn(formattedMessage);
    } else if (message.level >= LogLevel.Debug) {
      // eslint-disable-next-line no-console
      console.debug(formattedMessage);
    } else {
      // eslint-disable-next-line no-console
      console.log(formattedMessage);
    }
  }
}
