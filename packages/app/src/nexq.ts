import { RealTime, Store, Time } from "@nexq/core";
import { Rest } from "@nexq/proto-rest";
import { MemoryStore } from "@nexq/store-memory";
import fs from "node:fs";
import { parse as parseYaml } from "yaml";
import { ConfigParseError } from "./error/ConfigParseError.js";
import { NexqConfig, validateNexqConfig } from "./NexqConfig.js";

export interface StartOptions {
  configFilename: string;
}

export async function start(options: StartOptions): Promise<void> {
  const config = await loadConfig(options.configFilename);
  const time = new RealTime();
  const store = await createStore(config, time);
  await store.start();

  if (config.rest) {
    const rest = new Rest(store, config.rest);
    await rest.start();
  }
}

async function loadConfig(configFilename: string): Promise<NexqConfig> {
  const data = await fs.promises.readFile(configFilename, "utf8");
  let dataParsed: object;
  try {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    dataParsed = parseYaml(data);
  } catch (err) {
    throw new ConfigParseError(configFilename, err as Error);
  }
  const result = validateNexqConfig(dataParsed);
  if (result.success) {
    return result.data;
  } else {
    throw new ConfigParseError(configFilename, result.errors);
  }
}

async function createStore(config: NexqConfig, time: Time): Promise<Store> {
  switch (config.store.type) {
    case "memory":
      return await MemoryStore.create({ initialUser: config.initialUser, ...config.store, time });
    default:
      throw new Error(`unexpected store type ${config.store.type}`);
  }
}
