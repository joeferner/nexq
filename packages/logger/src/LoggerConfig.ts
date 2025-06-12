import { FormatterJsonConfig, formatterJsonConfigToFormatter } from "./formatter/config.js";
import { Formatter } from "./formatter/Formatter.js";
import { isLogLevel, LogLevel, LogLevelString, toLogLevel } from "./LogLevel.js";
import { TransportJsonConfig, transportJsonConfigToTransport } from "./transport/config.js";
import { Transport } from "./transport/Transport.js";
import { isString } from "./utils.js";

export interface LoggerConfig {
  level: LogLevel;
  appenders?: LoggerAppenderConfig[];
  logger?: Record<string, LoggerConfig>;
}

export interface LoggerAppenderConfig {
  level?: LogLevel;
  formatter?: Formatter;
  transport?: Transport;
}

function isLoggerConfig(config: LoggerConfig | LoggerJsonConfig): config is LoggerConfig {
  if (!isLogLevel(config.level)) {
    return false;
  }
  return true;
}

export interface LoggerJsonConfig {
  level: LogLevelString;
  appenders?: LoggerJsonAppenderConfig[];
  logger?: Record<string, LoggerJsonConfig | string>;
}

export interface LoggerJsonAppenderConfig {
  level?: LogLevelString;
  formatter?: FormatterJsonConfig;
  transport?: TransportJsonConfig;
}

function loggerJsonConfigToLoggerConfig(config: LoggerJsonConfig): LoggerConfig {
  let logger: Record<string, LoggerConfig> | undefined;
  if (config.logger) {
    logger = {};
    for (const loggerKey of Object.keys(config.logger)) {
      const loggerValue = config.logger[loggerKey];
      if (isString(loggerValue)) {
        logger[loggerKey] = {
          level: toLogLevel(loggerValue as LogLevelString),
        };
      } else {
        logger[loggerKey] = loggerJsonConfigToLoggerConfig(loggerValue);
      }
    }
  }

  return {
    level: toLogLevel(config.level),
    appenders: config.appenders?.map(appenderJsonConfigToAppender),
    logger,
  };
}

function appenderJsonConfigToAppender(config: LoggerJsonAppenderConfig): LoggerAppenderConfig {
  return {
    level: config.level ? toLogLevel(config.level) : undefined,
    formatter: config.formatter ? formatterJsonConfigToFormatter(config.formatter) : undefined,
    transport: config.transport ? transportJsonConfigToTransport(config.transport) : undefined,
  };
}

export function toLoggerConfig(config: LoggerConfig | LoggerJsonConfig): LoggerConfig {
  if (isLoggerConfig(config)) {
    return config;
  }
  return loggerJsonConfigToLoggerConfig(config);
}
