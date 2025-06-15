import fs from "node:fs";
import { Formatter, Message } from "../formatter/Formatter.js";
import { isString } from "../utils.js";
import { toTransportOptions, Transport, TransportOptions } from "./Transport.js";
import { TransportJsonConfigObj } from "./config.js";

export interface FileTransportOptions extends TransportOptions {
  filename: string;
}

function toFileTransportOptions(options: FileTransportOptions | TransportJsonConfigObj): FileTransportOptions {
  let filename: string;
  if ("filename" in options && isString(options.filename)) {
    filename = options.filename;
  } else {
    throw new Error('"filename" is a required option');
  }

  return {
    ...toTransportOptions(options),
    filename,
  };
}

export class FileTransport implements Transport {
  private readonly filename: string;

  public constructor(options: FileTransportOptions | TransportJsonConfigObj) {
    const opts = toFileTransportOptions(options);
    this.filename = opts.filename;
  }

  public log(message: Message): void {
    const formattedMessage = formatter.formatMessage(message, { enableColor: false });
    try {
      fs.appendFileSync(this.filename, `${formattedMessage}\n`);
    } catch (_err) {
      // failed to log
    }
  }
}
