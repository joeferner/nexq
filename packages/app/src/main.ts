/* eslint no-console: "off" */

import { command, run, string, option, multioption, array } from "cmd-ts";
import findRoot from "find-root";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";
import { start } from "./nexq.js";
import { ConfigParseError } from "./error/ConfigParseError.js";
import * as R from "radash";

async function parseCommandLineAndStart(): Promise<void> {
  const root = findRoot(fileURLToPath(import.meta.url));
  const packageJsonFilename = path.join(root, "package.json");
  // eslint-disable-next-line  @typescript-eslint/no-unsafe-assignment
  const packageJson: { version: string } = JSON.parse(await fs.promises.readFile(packageJsonFilename, "utf8"));
  const defaultConfigFilename = path.join(process.cwd(), "config/nexq.yml");

  const cmd = command({
    name: "nexq",
    description: "Queue server",
    version: packageJson.version,
    args: {
      config: option({
        short: "c",
        long: "config",
        type: string,
        description: "configuration file [default: ./config/nexq.yml]",
        defaultValue: () => defaultConfigFilename,
      }),
      configOverrides: multioption({
        short: 'D',
        long: 'D',
        type: array(string),
        description: "override a configuration value"
      })
    },
    handler: async (args) => {
      await start({ 
        ...args,
        configFilename: args.config
      });
    },
  });

  await run(cmd, process.argv.slice(2));
}

process.on("SIGINT", () => {
  // TODO shutdown gracefully
  process.exit(0);
});

process.on("SIGTERM", () => {
  // TODO shutdown gracefully
  process.exit(0);
});

parseCommandLineAndStart().catch((err) => {
  if (err instanceof ConfigParseError) {
    console.error(err.message);
    if (R.isArray(err.errors)) {
      for (const e of err.errors) {
        console.error(e);
      }
    } else {
      console.error(err.errors);
    }
  } else {
    console.error(err);
  }
  process.exit(1);
});
