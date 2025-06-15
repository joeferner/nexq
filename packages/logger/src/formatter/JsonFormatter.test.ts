import { describe, expect, test } from "vitest";
import { JsonFormatter } from "./JsonFormatter.js";
import { LogLevel } from "../LoggerConfig.js";

describe("JsonFormatter", () => {
  test("with formatting", () => {
    const formatter = new JsonFormatter({ format: false });
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
    ).toStrictEqual(`{"time":"2025-07-12T05:42:15.032Z","level":50,"message":"test","params":[]}`);
  });

  test("without formatting", () => {
    const formatter = new JsonFormatter({ format: true });
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
    ).toStrictEqual(`{
  "time": "2025-07-12T05:42:15.032Z",
  "level": 50,
  "message": "test",
  "params": []
}`);
  });
});
