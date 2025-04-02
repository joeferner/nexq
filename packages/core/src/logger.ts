/* eslint no-console: "off" */
/* eslint-disable @typescript-eslint/no-explicit-any */
/* eslint-disable @typescript-eslint/no-unsafe-argument */

export enum LogLevel {
  Debug = 0,
  Info = 10,
  Warn = 20,
  Error = 30,
  Off = 40,
}

export type LogLevelString = "debug" | "info" | "warn" | "error" | "off";

export interface LoggerConfig {
  defaultLevel?: LogLevelString;
  level?: Record<string, LogLevelString>;
}

export const DEFAULT_LOGGER_CONFIG: Required<LoggerConfig> = {
  defaultLevel: "info",
  level: {},
};

export class Logger {
  private static config: LoggerConfig = DEFAULT_LOGGER_CONFIG;
  private _level?: LogLevel;
  private _config?: LoggerConfig;

  public constructor(public readonly name: string) {}

  private get level(): LogLevel {
    if (this._config !== Logger.config) {
      this._level = undefined;
      this._config = Logger.config;
    }
    if (!this._level) {
      this._level = stringToLogLevel(
        Logger.config.level?.[this.name] ?? Logger.config.defaultLevel ?? DEFAULT_LOGGER_CONFIG.defaultLevel
      );
    }
    return this._level;
  }

  public static configure(config: LoggerConfig): void {
    Logger.config = {
      ...config,
      level: {
        ...DEFAULT_LOGGER_CONFIG.level,
        ...config.level,
      },
    };
  }

  public isDebugEnabled(): boolean {
    return this.level <= LogLevel.Debug;
  }

  public isInfoEnabled(): boolean {
    return this.level <= LogLevel.Info;
  }

  public isWarnEnabled(): boolean {
    return this.level <= LogLevel.Warn;
  }

  public isErrorEnabled(): boolean {
    return this.level <= LogLevel.Error;
  }

  public debug(message?: any, ...optionalParams: any[]): void {
    if (this.isDebugEnabled()) {
      console.debug(this.formatMessage("debug", message), ...optionalParams);
    }
  }

  public info(message?: any, ...optionalParams: any[]): void {
    if (this.isInfoEnabled()) {
      console.info(this.formatMessage("info", message), ...optionalParams);
    }
  }

  public warn(message?: any, ...optionalParams: any[]): void {
    if (this.isWarnEnabled()) {
      console.warn(this.formatMessage("warn", message), ...optionalParams);
    }
  }

  public error(message?: any, ...optionalParams: any[]): void {
    if (this.isErrorEnabled()) {
      console.error(this.formatMessage("error", message), ...optionalParams);
    }
  }

  private formatMessage(level: string, message: any): string {
    return `${new Date().toISOString()}: ${level}: ${this.name}: ${message}`;
  }
}

export function createLogger(name: string): Logger {
  return new Logger(name);
}

export function stringToLogLevel(logLevel: LogLevelString): LogLevel {
  switch (logLevel) {
    case "debug":
      return LogLevel.Debug;
    case "info":
      return LogLevel.Info;
    case "warn":
      return LogLevel.Warn;
    case "error":
      return LogLevel.Error;
    case "off":
      return LogLevel.Off;
    default:
      throw new Error(`unhandled log level "${logLevel}"`);
  }
}
