/* eslint no-console: "off" */

import { boolean, command, flag, option, optional, positional, run, string } from "cmd-ts";
import findRoot from "find-root";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { start } from "./tui.js";
import { getErrorMessage } from "./utils/error.js";

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
      url: positional({ type: string, displayName: "url" }),
    },
    handler: async (args) => {
      await start({
        ...args,
        rejectUnauthorized: !args.allowUnauthorized,
        ca: await readFile(args.ca),
        cert: await readFile(args.cert),
        key: await readFile(args.key),
        tuiVersion: packageJson.version,
      });
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
