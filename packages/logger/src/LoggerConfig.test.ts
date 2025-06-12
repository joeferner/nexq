import { describe, expect, test } from "vitest";
import { LoggerConfig, LoggerJsonConfig, toLoggerConfig } from "./LoggerConfig.js";
import { LogLevel } from "./LogLevel.js";
import { PatternFormatter } from "./formatter/PatternFormatter.js";
import { ConsoleTransport } from "./transport/ConsoleTransport.js";
import { FileTransport } from "./transport/FileTransport.js";
import path from "node:path";
import os from "node:os";

describe("LoggerConfig", () => {
  describe("toLoggerConfig", () => {
    test("no conversion needed", () => {
      const config: LoggerConfig = {
        level: LogLevel.Debug,
      };
      expect(toLoggerConfig(config)).toStrictEqual(config);
    });

    test("conversion needed", () => {
      const config: LoggerJsonConfig = {
        level: "debug",
      };
      const expected: LoggerConfig = {
        level: LogLevel.Debug,
        appenders: undefined,
        logger: undefined,
      };
      expect(toLoggerConfig(config)).toStrictEqual(expected);
    });

    test("formatter: pattern", () => {
      const jsonConfig: LoggerJsonConfig = {
        level: "debug",
        appenders: [{ formatter: "pattern" }],
      };
      const config = toLoggerConfig(jsonConfig);
      expect(config.appenders?.[0].formatter).toBeInstanceOf(PatternFormatter);
    });

    test("transport: console", () => {
      const jsonConfig: LoggerJsonConfig = {
        level: "debug",
        appenders: [{ transport: "console" }],
      };
      const config = toLoggerConfig(jsonConfig);
      expect(config.appenders?.[0].transport).toBeInstanceOf(ConsoleTransport);
    });

    test("transport: file", () => {
      const jsonConfig: LoggerJsonConfig = {
        level: "debug",
        appenders: [
          {
            transport: {
              name: "file",
              filename: path.join(os.tmpdir(), "test.log"),
            },
          },
        ],
      };
      const config = toLoggerConfig(jsonConfig);
      expect(config.appenders?.[0].transport).toBeInstanceOf(FileTransport);
    });
  });
});
