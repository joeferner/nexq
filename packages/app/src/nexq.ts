import { Logger, RealTime, Store, Time } from "@nexq/core";
import { Rest } from "@nexq/proto-rest";
import { MemoryStore } from "@nexq/store-memory";
import { SqlStore } from "@nexq/store-sql";
import fs from "node:fs";
import { parse as parseYaml } from "yaml";
import { ConfigParseError } from "./error/ConfigParseError.js";
import { MemoryStoreConfig, NexqConfig, SqlStoreConfig, validateNexqConfig } from "./NexqConfig.js";
import { DEFAULT_LOGGER_CONFIG } from "@nexq/core/build/logger.js";

export interface StartOptions {
  configFilename: string;
}

export async function start(options: StartOptions): Promise<void> {
  const config = await loadConfig(options.configFilename);
  Logger.configure(config.logger ?? DEFAULT_LOGGER_CONFIG);
  const time = new RealTime();
  const store = await createStore(config, time);

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
  const type = config.store.type;
  switch (config.store.type) {
    case "memory": {
      const storeConfig: MemoryStoreConfig = config.store;
      return await MemoryStore.create({ initialUser: config.initialUser, config: storeConfig, time });
    }
    case "sql": {
      const storeConfig: SqlStoreConfig = config.store;
      return await SqlStore.create({ initialUser: config.initialUser, config: storeConfig, time });
    }
    default:
      throw new Error(`unexpected store type ${type}`);
  }
}
