import { Logger, RealTime, Store, Time } from "@nexq/core";
import { DEFAULT_LOGGER_CONFIG } from "@nexq/core/build/logger.js";
import { PrometheusServer } from "@nexq/proto-prometheus";
import { RestServer } from "@nexq/proto-rest";
import { MemoryStore } from "@nexq/store-memory";
import { SqlStore } from "@nexq/store-sql";
import fs from "node:fs";
import { parse as parseYaml } from "yaml";
import { ConfigParseError } from "./error/ConfigParseError.js";
import { MemoryStoreConfig, NexqConfig, SqlStoreConfig, validateNexqConfig } from "./NexqConfig.js";

export interface StartOptions {
  configFilename: string;
}

export async function start(options: StartOptions): Promise<void> {
  const config = await loadConfig(options.configFilename);
  Logger.configure(config.logger ?? DEFAULT_LOGGER_CONFIG);
  const time = new RealTime();
  const store = await createStore(config, time);

  if (config.rest) {
    const rest = new RestServer(store, config.rest);
    await rest.start();
  }

  if (config.prometheus) {
    const prometheus = new PrometheusServer(store, config.prometheus);
    await prometheus.start();
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
