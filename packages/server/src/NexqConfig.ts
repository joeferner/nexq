import { CreateUserOptions, LoggerConfig } from "@nexq/core";
import typia from "typia";
import { MemoryStoreCreateConfig } from "@nexq/store-memory";
import { RestConfig } from "@nexq/proto-rest";
import { SqlStoreCreateConfig } from "@nexq/store-sql";
import { PrometheusConfig } from "@nexq/proto-prometheus";
import { KedaConfig } from "@nexq/proto-keda";

export type MemoryStoreConfig = { type: "memory" } & MemoryStoreCreateConfig;
export type SqlStoreConfig = { type: "sql" } & SqlStoreCreateConfig;

export interface NexqConfig {
  logger?: LoggerConfig;
  initialUsers?: CreateUserOptions[];
  store: MemoryStoreConfig | SqlStoreConfig;
  rest?: RestConfig;
  prometheus?: PrometheusConfig;
  keda?: KedaConfig;
}

export const validateNexqConfig = typia.createValidateEquals<NexqConfig>();
