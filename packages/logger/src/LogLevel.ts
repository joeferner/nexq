export type LogLevelString =
  | "trace"
  | "debug"
  | "info"
  | "notice"
  | "warn"
  | "error"
  | "critical"
  | "alert"
  | "emergency"
  | "off";

export enum LogLevel {
  Trace = 90,
  Debug = 80,
  Info = 70,
  Notice = 60,
  Warn = 50,
  Error = 40,
  Critical = 30,
  Alert = 20,
  Emergency = 10,
  Off = 0,
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
