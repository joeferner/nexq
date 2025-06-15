import { Transport } from "./transport/Transport.js";

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

export interface Config {
  level: LogLevel;
  appenders?: AppenderConfig[];
  filters?: FilterConfig[];
}

export interface AppenderConfig {
  level?: LogLevel;
  transport: Transport;
}

export interface FilterConfig {
  level?: LogLevel;
  namePattern?: string;
  transports?: Transport[];
}

export interface JsonConfig {
  level: LogLevelString | LogLevel;
  transports?: Record<string, JsonTransportConfig | string> | (string | JsonTransportConfig)[];
  rootTransports?: string[];
  filters?: Record<string, JsonFilterConfig | LogLevelString | LogLevel> | JsonFilterWithNamePatternConfig[];
}

export interface JsonTransportConfig {
  type: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [name: string]: any;
}

export interface JsonFilterConfig {
  level?: LogLevelString | LogLevel;
  transports?: string[];
}

export interface JsonFilterWithNamePatternConfig extends JsonFilterConfig {
  namePattern: string;
}
