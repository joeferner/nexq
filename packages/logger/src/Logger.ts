/* eslint-disable @typescript-eslint/no-unsafe-assignment */
/* eslint-disable @typescript-eslint/no-explicit-any */
import { PatternFormatter } from "./formatter/PatternFormatter.js";
import { LoggerAppenderConfig, LoggerConfig, LoggerJsonConfig, toLoggerConfig } from "./LoggerConfig.js";
import { Timer } from "./Timer.js";
import { LogLevel } from "./LogLevel.js";
import { ConsoleTransport } from "./transport/ConsoleTransport.js";
import { MessageContext } from "./MessageContext.js";

export interface ILogger {
  isDebugEnabled(): boolean;
  isInfoEnabled(): boolean;
  isNoticeEnabled(): boolean;
  isWarnEnabled(): boolean;
  isErrorEnabled(): boolean;
  isCriticalEnabled(): boolean;
  isAlertEnabled(): boolean;
  isEmergencyEnabled(): boolean;
  isEnabled(logLevel: LogLevel): boolean;

  log(level: LogLevel, message: any, ...params: any[]): void;
  debug(message?: any, ...params: any[]): void;
  info(message?: any, ...params: any[]): void;
  notice(message?: any, ...params: any[]): void;
  warn(message?: any, ...params: any[]): void;
  error(message?: any, ...params: any[]): void;
  critical(message?: any, ...params: any[]): void;
  alert(message?: any, ...params: any[]): void;
  emergency(message?: any, ...params: any[]): void;
}

export class Logger implements ILogger {
  private readonly name: string | undefined;
  private level: LogLevel;
  private appenders: Readonly<Readonly<Required<LoggerAppenderConfig>>[]>;
  private readonly children: Logger[] = [];

  public constructor(name?: string, parent?: Logger) {
    this.name = name;
    if (parent) {
      parent.children.push(this);
      this.appenders = parent.appenders;
    } else {
      this.appenders = [
        {
          level: LogLevel.Info,
          formatter: new PatternFormatter(),
          transport: new ConsoleTransport(),
        },
      ];
    }
    this.level = Math.max(...this.appenders.map((a) => a.level));
  }

  public configure(config: LoggerConfig | LoggerJsonConfig): void {
    const loggerConfig = toLoggerConfig(config);
    this.appenders = (loggerConfig.appenders ?? [{ level: loggerConfig.level }]).map((appender) => {
      return {
        level: appender.level ?? loggerConfig.level,
        formatter: appender.formatter ?? new PatternFormatter(),
        transport: appender.transport ?? new ConsoleTransport(),
      } satisfies Required<LoggerAppenderConfig>;
    });
    this.level = Math.max(...this.appenders.map((a) => a.level));

    for (const child of this.children) {
      if (!child.name) {
        continue;
      }
      const childConfig = loggerConfig.logger?.[child.name];
      child.configure(childConfig ?? config);
    }
  }

  public isDebugEnabled(): boolean {
    return this.isEnabled(LogLevel.Debug);
  }

  public isInfoEnabled(): boolean {
    return this.isEnabled(LogLevel.Info);
  }

  public isNoticeEnabled(): boolean {
    return this.isEnabled(LogLevel.Notice);
  }

  public isWarnEnabled(): boolean {
    return this.isEnabled(LogLevel.Warn);
  }

  public isErrorEnabled(): boolean {
    return this.isEnabled(LogLevel.Error);
  }

  public isCriticalEnabled(): boolean {
    return this.isEnabled(LogLevel.Critical);
  }

  public isAlertEnabled(): boolean {
    return this.isEnabled(LogLevel.Alert);
  }

  public isEmergencyEnabled(): boolean {
    return this.isEnabled(LogLevel.Emergency);
  }

  public isEnabled(logLevel: LogLevel): boolean {
    return this.level >= logLevel;
  }

  public log(level: LogLevel, message: any, ...params: any[]): void {
    if (this.level < level || this.appenders.length === 0) {
      return;
    }
    for (const appender of this.appenders) {
      if (appender.level < level) {
        continue;
      }

      let messageContext: MessageContext | undefined;
      if (params.length > 0 && params[params.length - 1] instanceof MessageContext) {
        messageContext = params[params.length - 1];
        params = params.slice(0, params.length - 1);
      }

      appender.transport.log(
        {
          time: new Date(),
          loggerName: this.name,
          level,
          message,
          params,
          messageContext
        },
        appender.formatter
      );
    }
  }

  public debug(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Debug, message, ...params);
  }

  public info(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Info, message, ...params);
  }

  public notice(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Notice, message, ...params);
  }

  public warn(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Warn, message, ...params);
  }

  public error(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Error, message, ...params);
  }

  public critical(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Critical, message, ...params);
  }

  public alert(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Alert, message, ...params);
  }

  public emergency(message?: any, ...params: any[]): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.log(LogLevel.Emergency, message, ...params);
  }

  public time(): Timer {
    return new Timer(this);
  }

  /**
   * Creates a logger as a child of this logger
   *
   * @param name The name of the new logger
   * @returns The child logger
   */
  public getLogger(name: string): Logger {
    const existingChild = this.children.find((c) => c.name === name);
    if (existingChild) {
      return existingChild;
    }
    return new Logger(name, this);
  }
}
