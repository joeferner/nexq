import { CreateStoreOptions, runStoreTest } from "@nexq/test";
import { describe, expect } from "vitest";
import { SqlStore } from "./SqlStore.js";
import fs from "node:fs";

describe("SqlStore", async () => {
  await runStoreTest((options: CreateStoreOptions) => {
    let filename = expect.getState().currentTestName?.replaceAll(/[>\s]+/g, "_");
    if (filename) {
      filename = `test-results/${filename}.sqlite`;
      try {
        fs.unlinkSync(filename);
      } catch (_err) {
        // ok
      }
      fs.mkdirSync("test-results", { recursive: true });
    }
    return SqlStore.create({
      ...options,
      passwordHashRounds: 1,
      config: { pollInterval: 0, dialect: "sqlite", connectionString: filename ?? ":memory:" },
    });
  });
});
