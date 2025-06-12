/* eslint no-console: "off" */

import { FileTransport, logger, LoggerAppenderConfig, LogLevel } from "@nexq/logger";
import { boolean, command, flag, option, optional, positional, run, string } from "cmd-ts";
import findRoot from "find-root";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { start } from "./tui.js";
import { getErrorMessage } from "./utils/error.js";

export const DEFAULT_LOG_FILE = path.join(os.tmpdir(), "nexq-tui.log");
const log = logger.getLogger("main");

async function parseCommandLineAndStart(): Promise<void> {
  const root = findRoot(fileURLToPath(import.meta.url));
  const packageJsonFilename = path.join(root, "package.json");
  // eslint-disable-next-line  @typescript-eslint/no-unsafe-assignment
  const packageJson: { version: string } = JSON.parse(await fs.promises.readFile(packageJsonFilename, "utf8"));

  const cmd = command({
    name: "nexq",
    description: "NexQ TUI",
    version: packageJson.version,
    args: {
      allowUnauthorized: flag({
        long: "allowUnauthorized",
        type: boolean,
        description: "disable strict certificate validation",
        defaultValue: () => false,
      }),
      ca: option({
        long: "ca",
        type: optional(string),
        description: "Certificate authority file",
      }),
      cert: option({
        long: "cert",
        type: optional(string),
        description: "Certificate used to connect",
      }),
      key: option({
        long: "key",
        type: optional(string),
        description: "Private key used to connect",
      }),
      logFile: option({
        long: "log-file",
        type: optional(string),
        defaultValue: () => DEFAULT_LOG_FILE,
        description: "File to write log messages to",
      }),
      debug: flag({
        long: "debug",
        type: boolean,
        defaultValue: () => false,
        description: "Increases verbosity of logging",
      }),
      url: positional({ type: string, displayName: "url" }),
    },
    handler: async (args) => {
      try {
        const appenders: LoggerAppenderConfig[] = [];
        if (args.logFile) {
          const logFile = args.logFile;
          appenders.push({
            transport: new FileTransport({ filename: logFile }),
          });
        }
        logger.configure({
          level: args.debug ? LogLevel.Debug : LogLevel.Info,
          appenders,
        });

        try {
          new URL(args.url);
        } catch (_err) {
          console.error("invalid url");
          process.exit(1);
        }

        await start({
          ...args,
          rejectUnauthorized: !args.allowUnauthorized,
          ca: await readFile(args.ca),
          cert: await readFile(args.cert),
          key: await readFile(args.key),
          tuiVersion: packageJson.version,
        });
      } catch (err) {
        log.error("start failed", err);
        console.error(getErrorMessage(err));
      }
    },
  });

  await run(cmd, process.argv.slice(2));
}

async function readFile(filename: string | undefined): Promise<Buffer | undefined> {
  if (!filename) {
    return undefined;
  }
  try {
    return await fs.promises.readFile(filename);
  } catch (err) {
    throw new Error(`failed to read file: ${filename}: ${getErrorMessage(err)}`);
  }
}

parseCommandLineAndStart().catch((err) => {
  console.error(err);
  process.exit(1);
});
