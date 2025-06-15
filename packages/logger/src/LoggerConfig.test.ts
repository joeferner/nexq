import os from 'node:os';
import path from 'node:path';
import { describe, expect, test } from "vitest";
import { Config, JsonConfig, LogLevel, } from "./LoggerConfig.js";
import { toConfig } from "./LoggerConfig.utils.js";
import { ConsoleTransport } from "./transport/ConsoleTransport.js";
import { FileTransport } from "./transport/FileTransport.js";

describe("LoggerConfig", () => {
  describe("toConfig", () => {
    const filename = path.join(os.tmpdir(), 'LoggerConfig.ts');

    function validateToConfig(config: JsonConfig, expected: Required<Config>): void {
      expect(toConfig(config)).toStrictEqual(expected);
    }

    test("no level conversion needed", () => {
      const config: JsonConfig = {
        level: LogLevel.Debug,
      };
      const expected: Required<Config> = {
        level: LogLevel.Debug,
        appenders: [{
          transport: new ConsoleTransport()
        }],
        filters: []
      };
      validateToConfig(config, expected);
    });

    test("level conversion", () => {
      const config: JsonConfig = {
        level: "debug",
      };
      const expected: Required<Config> = {
        level: LogLevel.Debug,
        appenders: [{
          transport: new ConsoleTransport()
        }],
        filters: []
      };
      validateToConfig(config, expected);
    });

    test("transports array", () => {
      const config: JsonConfig = {
        level: "debug",
        transports: ['console', { type: 'file', filename }]
      };
      const expected: Required<Config> = {
        level: LogLevel.Debug,
        appenders: [{
          transport: new ConsoleTransport()
        }, {
          transport: new FileTransport({ filename })
        }],
        filters: []
      };
      validateToConfig(config, expected);
    });

    test("filter: record", () => {
      const jsonConfig: JsonConfig = {
        level: "debug",
        transports: [{ name: 'console', type: 'console' }],
        filters: {
          'testDebug': 'debug',
          'testInfo': LogLevel.Info,
          'testWarn': {
            level: LogLevel.Warn,
            transports: ['console']
          }
        }
      };
      const config = toConfig(jsonConfig);
      expect(Object.keys(config.filters).length).toBe(0);
      expect(config.appenders.length).toBe(1);
      expect(config.appenders[0].level).toBeUndefined();
      expect(config.appenders[0].transport).toBeInstanceOf(ConsoleTransport);

      expect(config.filters.length).toBe(3);

      expect(config.filters[0].namePattern).toBe('testDebug');
      expect(config.filters[0].level).toBe(LogLevel.Debug);
      expect(config.filters[0].transports).toBeUndefined();

      expect(config.filters[0].namePattern).toBe('testInfo');
      expect(config.filters[0].level).toBe(LogLevel.Info);
      expect(config.filters[0].transports).toBeUndefined();

      expect(config.filters[0].namePattern).toBe('testWarn');
      expect(config.filters[0].level).toBe(LogLevel.Warn);
      expect(config.filters[0].transports?.length).toBe(1);
      expect(config.filters[0].transports?.[0]).toStrictEqual(config.appenders[0].transport);
    });
  });
});
