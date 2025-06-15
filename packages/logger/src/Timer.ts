/* eslint-disable @typescript-eslint/no-explicit-any */
import { ILogger, Logger } from "./Logger.js";
import { LogLevel } from "./LoggerConfig.js";

export class Timer implements ILogger {
  private startTime: number;

  public constructor(private readonly logger: Logger) {
    this.startTime = Date.now();
  }

  public get duration(): number {
    return Date.now() - this.startTime;
  }

  public reset(): void {
    this.startTime = Date.now();
  }

  public get durationString(): string {
    const duration = this.duration;
    return `${duration}ms`;
  }

  public isDebugEnabled(): boolean {
    return this.logger.isDebugEnabled();
  }

  public isInfoEnabled(): boolean {
    return this.logger.isInfoEnabled();
  }

  public isNoticeEnabled(): boolean {
    return this.logger.isNoticeEnabled();
  }

  public isWarnEnabled(): boolean {
    return this.logger.isWarnEnabled();
  }

  public isErrorEnabled(): boolean {
    return this.logger.isErrorEnabled();
  }

  public isCriticalEnabled(): boolean {
    return this.logger.isCriticalEnabled();
  }

  public isAlertEnabled(): boolean {
    return this.logger.isAlertEnabled();
  }

  public isEmergencyEnabled(): boolean {
    return this.logger.isEmergencyEnabled();
  }

  public isEnabled(logLevel: LogLevel): boolean {
    return this.logger.isEnabled(logLevel);
  }

  public log(level: LogLevel, message: any, ...params: any[]): void {
    message = `${message} (${this.durationString})`;
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    this.logger.log(level, message, ...params);
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
}
