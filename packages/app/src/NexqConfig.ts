import { CreateUserOptions } from "@nexq/core";
import typia from "typia";
import { MemoryStoreCreateConfig } from "@nexq/store-memory";
import { RestConfig } from "@nexq/proto-rest";
import { SqlStoreCreateConfig } from "@nexq/store-sql";

export type MemoryStoreConfig = { type: "memory" } & MemoryStoreCreateConfig;
export type SqlStoreConfig = { type: "sql" } & SqlStoreCreateConfig;

export interface NexqConfig {
  initialUser?: CreateUserOptions;
  store: MemoryStoreConfig | SqlStoreConfig;
  rest?: RestConfig;
}

export const validateNexqConfig = typia.createValidateEquals<NexqConfig>();
