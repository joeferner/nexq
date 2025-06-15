/* eslint-disable @typescript-eslint/no-unsafe-member-access */
/* eslint-disable @typescript-eslint/no-unsafe-assignment */
/* eslint-disable @typescript-eslint/no-explicit-any */
/* eslint-disable no-console */
import { describe, expect, test } from "vitest";
import { Message } from "../formatter/Formatter.js";
import { PatternFormatter } from "../formatter/PatternFormatter.js";
import { LogLevel } from "../LoggerConfig.js";
import { logLevelToString } from "../LoggerConfig.utils.js";
import { ConsoleTransport } from "./ConsoleTransport.js";

describe("ConsoleTransport", () => {
  type ConsoleFunction = "debug" | "log" | "warn" | "error";
  const fnToHook: ConsoleFunction[] = ["debug", "log", "warn", "error"];
  const transport = new ConsoleTransport({ enableColor: false });
  const message: Message = {
    time: new Date(2025, 6, 12, 5, 42, 15, 32),
    level: LogLevel.Info,
    message: "test1",
    params: [],
  };
  const formatter = new PatternFormatter();

  function withMockConsole(fn: () => void): Record<ConsoleFunction, string[]> {
    const messages: Record<ConsoleFunction, string[]> = {
      debug: [],
      error: [],
      log: [],
      warn: [],
    };
    const oldFn: Record<ConsoleFunction, any> = {
      debug: console.debug,
      log: console.log,
      warn: console.warn,
      error: console.error,
    };
    for (const name of fnToHook) {
      (console as any)[name] = (message: string): void => {
        messages[name].push(message);
      };
    }
    try {
      fn();
    } finally {
      for (const name of fnToHook) {
        (console as any)[name] = oldFn[name];
      }
    }
    return messages;
  }

  function testLevel(logLevel: LogLevel, expectedConsoleFn: string): void {
    test(`${logLevelToString(logLevel)}`, () => {
      const messages = withMockConsole(() => {
        transport.log(
          {
            ...message,
            level: logLevel,
          },
          formatter
        );
      });
      for (const name of fnToHook) {
        if (expectedConsoleFn === name) {
          expect(messages[name], `expected "${name}" to have entry`).toStrictEqual([
            `2025-07-12T05:42:15.032Z: ${logLevelToString(logLevel)}: test1`,
          ]);
        } else {
          expect(messages[name], `expected "${name}" to be empty`).toEqual([]);
        }
      }
    });
  }

  const expectedLogFunction: Record<LogLevel, ConsoleFunction> = {
    [LogLevel.Trace]: "debug",
    [LogLevel.Debug]: "debug",
    [LogLevel.Info]: "log",
    [LogLevel.Notice]: "log",
    [LogLevel.Warn]: "warn",
    [LogLevel.Error]: "error",
    [LogLevel.Critical]: "error",
    [LogLevel.Alert]: "error",
    [LogLevel.Emergency]: "error",
    [LogLevel.Off]: "debug",
  };
  for (const logLevelValue of Object.keys(expectedLogFunction)) {
    const logLevel = parseInt(logLevelValue) as LogLevel;
    if (logLevel === LogLevel.Off) {
      continue;
    }
    testLevel(logLevel, expectedLogFunction[logLevel] as any as ConsoleFunction);
  }
});
