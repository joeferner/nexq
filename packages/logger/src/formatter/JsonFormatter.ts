import { toBoolean } from "../utils.js";
import { FormatterJsonConfigObj } from "./config.js";
import { Formatter, FormatterMessageOptions, FormatterOptions, Message, toFormatterOptions } from "./Formatter.js";

export interface JsonFormatterOptions extends FormatterOptions {
  format?: boolean;
}

function toJsonFormatterOptions(options?: JsonFormatterOptions | FormatterJsonConfigObj): JsonFormatterOptions {
  if (options === undefined) {
    return {};
  }

  let format: boolean | undefined;
  if (options.format !== undefined) {
    try {
      format = toBoolean(options.format);
    } catch (err) {
      throw new Error('"format" is invalid', { cause: err });
    }
  } else {
    format = undefined;
  }

  return {
    ...toFormatterOptions(options),
    format,
  };
}

export class JsonFormatter implements Formatter {
  private readonly format: boolean;

  public constructor(options?: JsonFormatterOptions | FormatterJsonConfigObj) {
    const opts = toJsonFormatterOptions(options);
    this.format = opts.format ?? false;
  }

  public formatMessage(message: Message, _options: FormatterMessageOptions): string {
    if (this.format) {
      return JSON.stringify(message, null, 2);
    } else {
      return JSON.stringify(message);
    }
  }
}
