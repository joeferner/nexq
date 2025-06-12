import { isString } from "../utils.js";
import { ConsoleTransport } from "./ConsoleTransport.js";
import { FileTransport } from "./FileTransport.js";
import { Transport } from "./Transport.js";

export interface TransportJsonConfigObj {
  name: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [key: string]: any;
}

export type TransportJsonConfig = string | TransportJsonConfigObj;

export function transportJsonConfigToTransport(transportJson: TransportJsonConfig): Transport {
  if (isString(transportJson)) {
    return transportJsonConfigToTransport({ name: transportJson });
  }
  if (transportJson.name === "console") {
    return new ConsoleTransport(transportJson);
  }
  if (transportJson.name === "file") {
    return new FileTransport(transportJson);
  }
  throw new Error(`Unknown transport "${transportJson.name}"`);
}
