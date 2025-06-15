import { formatterJsonConfigToFormatter } from "./formatter/config.js";
import { AppenderConfig, Config, JsonConfig, JsonTransportConfig, LogLevel, LogLevelString } from "./LoggerConfig.js";
import { transportJsonConfigToTransport } from "./transport/config.js";
import { Transport } from "./transport/Transport.js";

export function toConfig(config: Config | JsonConfig): Required<Config> {
  return {
    level: toLogLevel(config.level),
    appenders,
    filters
  }
}

function jsonConfigToConfig(config: JsonConfig): Required<Config> {
  const transports: Record<string, Transport> = {};
  for (const [transportName, transportConfig] of Object.entries(config.transports ?? {})) {
    transports[transportName] = jsonTransportConfigToTransport(transportConfig);
  }

  return {
    level: toLogLevel(config.level),
    appenders: transports,
    logger,
  };
}

function jsonTransportConfigToTransport(transport: JsonTransportConfig): Transport {

}

const LOG_LEVEL_TO_STRING: Readonly<Record<LogLevel, LogLevelString>> = {
  [LogLevel.Trace]: "trace",
  [LogLevel.Debug]: "debug",
  [LogLevel.Info]: "info",
  [LogLevel.Notice]: "notice",
  [LogLevel.Warn]: "warn",
  [LogLevel.Error]: "error",
  [LogLevel.Critical]: "critical",
  [LogLevel.Alert]: "alert",
  [LogLevel.Emergency]: "emergency",
  [LogLevel.Off]: "off",
};

const STRING_TO_LOG_LEVEL: Readonly<Record<LogLevelString, LogLevel>> = {
  trace: LogLevel.Trace,
  debug: LogLevel.Debug,
  info: LogLevel.Info,
  notice: LogLevel.Notice,
  warn: LogLevel.Warn,
  error: LogLevel.Error,
  critical: LogLevel.Critical,
  alert: LogLevel.Alert,
  emergency: LogLevel.Emergency,
  off: LogLevel.Off,
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function isLogLevel(logLevel: any): logLevel is LogLevel {
  // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
  return Object.values(LogLevel).includes(logLevel);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function isLogLevelString(logLevel: any): logLevel is LogLevelString {
  // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
  return Object.values(LOG_LEVEL_TO_STRING).includes(logLevel);
}

export function logLevelToString(logLevel: LogLevelString | LogLevel): string {
  if (isLogLevel(logLevel)) {
    return LOG_LEVEL_TO_STRING[logLevel];
  }

  return logLevel;
}

export function toLogLevel(logLevel: LogLevelString | LogLevel): LogLevel {
  if (isLogLevel(logLevel)) {
    return logLevel;
  }

  const level = STRING_TO_LOG_LEVEL[logLevel.toLocaleLowerCase() as LogLevelString];
  if (level) {
    return level;
  }

  throw new Error(`unhandled log level "${logLevel}"`);
}
