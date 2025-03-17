import { CreateUserOptions, LoggerConfig } from "@nexq/core";
import typia from "typia";
import { MemoryStoreCreateConfig } from "@nexq/store-memory";
import { RestConfig } from "@nexq/proto-rest";
import { SqlStoreCreateConfig } from "@nexq/store-sql";
import { PrometheusConfig } from "@nexq/proto-prometheus";

export type MemoryStoreConfig = { type: "memory" } & MemoryStoreCreateConfig;
export type SqlStoreConfig = { type: "sql" } & SqlStoreCreateConfig;

export interface NexqConfig {
  logger?: LoggerConfig;
  initialUser?: CreateUserOptions;
  store: MemoryStoreConfig | SqlStoreConfig;
  rest?: RestConfig;
  prometheus?: PrometheusConfig;
}

export const validateNexqConfig = typia.createValidateEquals<NexqConfig>();
