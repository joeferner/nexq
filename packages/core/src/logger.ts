/* eslint-disable @typescript-eslint/no-explicit-any */
/* eslint-disable @typescript-eslint/no-unsafe-argument */

export class Logger {
  public constructor(public readonly name: string) {}

  public isDebugEnabled(): boolean {
    return true;
  }

  public debug(message?: any, ...optionalParams: any[]): void {
    if (this.isDebugEnabled()) {
      console.debug(this.formatMessage("debug", message), ...optionalParams);
    }
  }

  public info(message?: any, ...optionalParams: any[]): void {
    console.info(this.formatMessage("info", message), ...optionalParams);
  }

  public error(message?: any, ...optionalParams: any[]): void {
    console.error(this.formatMessage("error", message), ...optionalParams);
  }

  private formatMessage(level: string, message: any): string {
    return `${new Date().toISOString()}: ${level}: ${this.name}: ${message}`;
  }
}

export function createLogger(name: string): Logger {
  return new Logger(name);
}
