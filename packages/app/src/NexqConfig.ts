import { CreateUserOptions } from "@nexq/core";
import typia from "typia";
import { MemoryCreateConfig } from "@nexq/store-memory";
import { RestConfig } from "@nexq/proto-rest";

export type MemoryStore = { type: "memory" } & MemoryCreateConfig;

export interface NexqConfig {
  initialUser?: CreateUserOptions;
  store: MemoryStore;
  rest?: RestConfig;
}

export const validateNexqConfig = typia.createValidateEquals<NexqConfig>();
