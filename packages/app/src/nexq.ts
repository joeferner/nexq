/* eslint no-console: "off" */

import { createLogger, DEFAULT_LOGGER_CONFIG, Logger, RealTime, Store, Time } from "@nexq/core";
import { KedaServer } from "@nexq/proto-keda";
import { PrometheusServer } from "@nexq/proto-prometheus";
import { RestServer } from "@nexq/proto-rest";
import { MemoryStore } from "@nexq/store-memory";
import { SqlStore } from "@nexq/store-sql";
import fs from "node:fs";
import path from "node:path";
import { parse as parseYaml } from "yaml";
import { ConfigParseError } from "./error/ConfigParseError.js";
import { MemoryStoreConfig, NexqConfig, SqlStoreConfig, validateNexqConfig } from "./NexqConfig.js";
import { applyConfigOverrides, envSubstitution } from "./utils.js";

const logger = createLogger("App");

export interface StartOptions {
  configFilename: string;
  configOverrides?: string[];
}

export async function start(options: StartOptions): Promise<void> {
  const fullConfigFilename = path.resolve(options.configFilename);
  const config = await loadConfig(fullConfigFilename, options.configOverrides);
  Logger.configure(config.logger ?? DEFAULT_LOGGER_CONFIG);
  const logger = createLogger("NexQ");
  logger.info(`using config "${fullConfigFilename}"`);
  const time = new RealTime();
  const store = await createStore(config, time);
  void store.poll();

  if (config.rest) {
    const rest = new RestServer(store, config.rest);
    await rest.start();
  }

  if (config.prometheus) {
    const prometheus = new PrometheusServer(store, config.prometheus);
    await prometheus.start();
  }

  if (config.keda) {
    const keda = new KedaServer(store, config.keda);
    await keda.start();
  }
}

async function loadConfig(configFilename: string, configOverrides?: string[]): Promise<NexqConfig> {
  if (!fs.existsSync(configFilename)) {
    console.error(`config file "${configFilename}" does not exist`);
    process.exit(1);
  }

  let data: string;
  try {
    data = await fs.promises.readFile(configFilename, "utf8");
  } catch (err) {
    console.error(`failed to read config file "${configFilename}": ${err as Error}`);
    process.exit(1);
  }
  data = envSubstitution(data);

  let dataParsed: object;
  try {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    dataParsed = parseYaml(data);
  } catch (err) {
    throw new ConfigParseError(configFilename, err as Error);
  }
  applyConfigOverrides(dataParsed, configOverrides);
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
      logger.info("Using memory store");
      return await MemoryStore.create({ initialUsers: config.initialUsers, config: storeConfig, time });
    }
    case "sql": {
      const storeConfig: SqlStoreConfig = config.store;
      logger.info(
        `Using sql store: ${storeConfig.type}: ${SqlStore.maskPasswordInConnectionString(storeConfig.connectionString)}`
      );
      return await SqlStore.create({ initialUsers: config.initialUsers, config: storeConfig, time });
    }
    default:
      throw new Error(`unexpected store type ${type}`);
  }
}
