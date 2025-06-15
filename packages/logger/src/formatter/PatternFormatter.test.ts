import { describe, expect, test } from "vitest";
import { PatternFormatter } from "./PatternFormatter.js";
import { LogLevel } from "../LoggerConfig.js";

describe("PatternFormatter", () => {
  const formatter = new PatternFormatter();

  test("with color", () => {
    expect(
      formatter.formatMessage(
        {
          time: new Date(2025, 6, 12, 5, 42, 15, 32),
          level: LogLevel.Warn,
          message: "test",
          params: [],
        },
        { enableColor: true }
      )
    ).toStrictEqual("2025-07-12T05:42:15.032Z: \x1b[33mwarn\x1b[0m: test");
  });

  test("without color", () => {
    expect(
      formatter.formatMessage(
        {
          time: new Date(2025, 6, 12, 5, 42, 15, 32),
          level: LogLevel.Warn,
          message: "test",
          params: [],
        },
        { enableColor: false }
      )
    ).toStrictEqual("2025-07-12T05:42:15.032Z: warn: test");
  });

  test("logger name", () => {
    expect(
      formatter.formatMessage(
        {
          time: new Date(2025, 6, 12, 5, 42, 15, 32),
          level: LogLevel.Warn,
          message: "test",
          params: [],
          loggerName: "log1",
        },
        { enableColor: false }
      )
    ).toStrictEqual("2025-07-12T05:42:15.032Z: warn: log1: test");
  });

  test("format error", () => {
    const message = formatter.formatMessage(
      {
        time: new Date(2025, 6, 12, 5, 42, 15, 32),
        level: LogLevel.Warn,
        message: "test",
        params: [new Error("test error")],
      },
      { enableColor: false }
    );
    const lines = message.split("\n");
    expect(lines[0]).toStrictEqual(`2025-07-12T05:42:15.032Z: warn: test Error: test error`);
    expect(lines.length).toBeGreaterThan(1);
    for (let i = 1; i < lines.length; i++) {
      expect(lines[i].trim().startsWith("at"), `expected "${lines[i]}" to start with "at"`).toBeTruthy();
    }
  });

  test("format object", () => {
    const message = formatter.formatMessage(
      {
        time: new Date(2025, 6, 12, 5, 42, 15, 32),
        level: LogLevel.Warn,
        message: "test",
        params: [{ str: "value", num: 42 }],
      },
      { enableColor: false }
    );
    expect(message).toStrictEqual(`2025-07-12T05:42:15.032Z: warn: test {
  "str": "value",
  "num": 42
}`);
  });

  test("format multiple parameters", () => {
    const message = formatter.formatMessage(
      {
        time: new Date(2025, 6, 12, 5, 42, 15, 32),
        level: LogLevel.Warn,
        message: "test",
        params: ["param1", 42, { test: 12 }],
      },
      { enableColor: false }
    );
    expect(message).toStrictEqual(`2025-07-12T05:42:15.032Z: warn: test param1 42 {
  "test": 12
}`);
  });
});
