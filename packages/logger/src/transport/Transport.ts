import { Message } from "../formatter/Formatter.js";
import { TransportJsonConfigObj } from "./config.js";

export type TransportOptions = object;

export function toTransportOptions(_options: TransportJsonConfigObj | TransportOptions): TransportOptions {
  return {};
}

export interface Transport {
  log(message: Message): void;
}
