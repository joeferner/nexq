import { Logger } from "./Logger.js";

export * from "./formatter/index.js";
export * from "./transport/index.js";
export { ILogger, Logger } from "./Logger.js";
export * from "./LoggerConfig.js";
export { Timer } from "./Timer.js";

/**
 * Root logger
 */
export const logger = new Logger();
