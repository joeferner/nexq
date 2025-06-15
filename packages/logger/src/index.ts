import { Logger } from "./Logger.js";

export * from "./formatter/index.js";
export * from "./transport/index.js";
export { ILogger, Logger } from "./Logger.js";
export { LoggerConfig, LoggerAppenderConfig, LoggerJsonConfig, LoggerJsonAppenderConfig } from "./LoggerConfig.js";
export { Timer } from "./Timer.js";
export { LogLevel, LogLevelString, toLogLevel } from "./LogLevel.js";

/**
 * Root logger
 */
export const logger = new Logger(undefined, undefined);
