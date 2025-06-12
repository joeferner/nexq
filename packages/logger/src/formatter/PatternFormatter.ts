import { LogLevel, logLevelToString } from "../LogLevel.js";
import { anyToString, toString } from "../utils.js";
import { FormatterJsonConfigObj } from "./config.js";
import { Formatter, FormatterMessageOptions, FormatterOptions, Message, toFormatterOptions } from "./Formatter.js";

const DEFAULT_PATTERN = "${timeISOString}: ${level}:${notEmpty( ${name}:)} ${message}";
const COLOR_GREEN = "\x1b[32m";
const COLOR_YELLOW = "\x1b[33m";
const COLOR_DEFAULT = "\x1b[0m";
const COLOR_RED = "\x1b[31m";

export interface PatternFormatterOptions extends FormatterOptions {
  pattern?: string;
}

function toPatternFormatterOptions(
  options?: PatternFormatterOptions | FormatterJsonConfigObj
): PatternFormatterOptions {
  if (options === undefined) {
    return {};
  }

  let pattern: string | undefined;
  if (options.pattern !== undefined) {
    try {
      pattern = toString(options.pattern);
    } catch (err) {
      throw new Error('"pattern" is invalid', { cause: err });
    }
  } else {
    pattern = undefined;
  }

  return {
    ...toFormatterOptions(options),
    pattern,
  };
}

export type FormatFunction = (message: Message, options: FormatterMessageOptions) => string | undefined;

export class PatternFormatter implements Formatter {
  private readonly formatFunctions: FormatFunction[];

  public constructor(options?: PatternFormatterOptions | FormatterJsonConfigObj) {
    const opts = toPatternFormatterOptions(options);
    this.formatFunctions = parsePatternToFunctions(opts?.pattern ?? DEFAULT_PATTERN);
  }

  public formatMessage(message: Message, options: FormatterMessageOptions): string {
    return this.formatFunctions.map((fn) => fn(message, options)).join("");
  }
}

function createLiteralTextFormatFunction(text: string): FormatFunction {
  return (): string => {
    return text;
  };
}

function createTimeISOStringFormatFunction(): FormatFunction {
  return (message: Message): string => {
    return message.time.toISOString();
  };
}

function createNameFormatFunction(): FormatFunction {
  return (message: Message): string | undefined => {
    return message.loggerName;
  };
}

function createLevelFormatFunction(): FormatFunction {
  const logLevelToColor: Record<LogLevel, string> = {
    [LogLevel.Trace]: COLOR_DEFAULT,
    [LogLevel.Debug]: COLOR_DEFAULT,
    [LogLevel.Info]: COLOR_GREEN,
    [LogLevel.Notice]: COLOR_YELLOW,
    [LogLevel.Warn]: COLOR_YELLOW,
    [LogLevel.Error]: COLOR_RED,
    [LogLevel.Critical]: COLOR_RED,
    [LogLevel.Alert]: COLOR_RED,
    [LogLevel.Emergency]: COLOR_RED,
    [LogLevel.Off]: COLOR_DEFAULT,
  };

  return (message: Message, options: FormatterMessageOptions): string => {
    if (options.enableColor) {
      const color = logLevelToColor[message.level];
      return `${color}${logLevelToString(message.level)}${COLOR_DEFAULT}`;
    } else {
      return logLevelToString(message.level);
    }
  };
}

function createMessageFormatFunction(): FormatFunction {
  return (message: Message): string => {
    let result = anyToString(message.message);
    if (message.params.length > 0) {
      const paramsString = message.params.map(anyToString).join(" ");
      result += " " + paramsString;
    }
    return result;
  };
}

/**
 * Outputs the result of evaluating the pattern if and only if all variables in the pattern are not empty.
 */
function createNotEmptyFormatFunction(expr: string): FormatFunction {
  if (!expr.startsWith("notEmpty(") || !expr.endsWith(")")) {
    throw new Error(`Invalid notEmpty expression "${expr}"`);
  }
  let subFunctions: FormatFunction[];
  const arg = expr.substring("notEmpty(".length, expr.length - ")".length);
  try {
    subFunctions = parsePatternToFunctions(arg);
  } catch (err) {
    throw new Error(`Invalid notEmpty argument "${arg}"`, { cause: err });
  }

  return (message: Message, options: FormatterMessageOptions): string | undefined => {
    const parts: string[] = [];
    for (const subFunction of subFunctions) {
      const part = subFunction(message, options);
      if (part === undefined) {
        return undefined;
      }
      parts.push(part);
    }
    return parts.join("");
  };
}

function parsePatternToFunctions(pattern: string): FormatFunction[] {
  const parts = splitPatternIntoParts(pattern);
  const formatFunctions: FormatFunction[] = [];
  for (const part of parts) {
    if (part.startsWith("${") && part.endsWith("}")) {
      const value = part.substring("${".length, part.length - "}".length);
      if (value === "timeISOString") {
        formatFunctions.push(createTimeISOStringFormatFunction());
      } else if (value === "level") {
        formatFunctions.push(createLevelFormatFunction());
      } else if (value === "name") {
        formatFunctions.push(createNameFormatFunction());
      } else if (value === "message") {
        formatFunctions.push(createMessageFormatFunction());
      } else if (value.startsWith("notEmpty")) {
        formatFunctions.push(createNotEmptyFormatFunction(value));
      } else {
        throw new Error(`unknown pattern substitute "${value}"`);
      }
    } else {
      formatFunctions.push(createLiteralTextFormatFunction(part));
    }
  }
  return formatFunctions;
}

function splitPatternIntoParts(pattern: string): string[] {
  let text = "";
  const parts: string[] = [];

  let i = 0;
  while (i < pattern.length) {
    if (pattern[i] === "$" && pattern[i + 1] === "{") {
      if (text.length > 0) {
        parts.push(text);
        text = "";
      }
      text = "${";
      i += 2;
      let nesting = 0;
      while (i < pattern.length) {
        if (pattern[i] === "$" && pattern[i + 1] === "{") {
          nesting++;
          text += "${";
          i += 2;
        }
        if (pattern[i] === "}") {
          text += "}";
          i++;
          if (nesting === 0) {
            parts.push(text);
            text = "";
            break;
          } else {
            nesting--;
          }
        } else {
          text += pattern[i++];
        }
      }
    } else {
      text += pattern[i++];
    }
  }
  if (text.length > 0) {
    parts.push(text);
  }
  return parts;
}
