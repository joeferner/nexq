import { runStoreTest, CreateStoreOptions } from "@nexq/test";
import { MemoryStore } from "./MemoryStore.js";
import { describe } from "vitest";

describe("MemoryStore", async () => {
  await runStoreTest((options: CreateStoreOptions) => {
    return MemoryStore.create({ ...options, passwordHashRounds: 1, config: { pollInterval: "30s" } });
  });
});
