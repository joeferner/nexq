import { CreateStoreOptions, runStoreTest } from "@nexq/test";
import fs from "node:fs";
import { describe, expect } from "vitest";
import { SqlStore, SqlStoreCreateConfig, SqlStoreCreateConfigPostgres } from "./SqlStore.js";

describe("SqlStore", async () => {
  await runStoreTest(async (options: CreateStoreOptions) => {
    let config: SqlStoreCreateConfig;

    if (process.env["TEST_POSTGRES"]) {
      config = {
        dialect: "postgres",
        pollInterval: 0,
        connectionString: "postgres://nexq-postgres/nexq",
        options: {
          user: "nexq",
          password: "password",
        },
      } satisfies SqlStoreCreateConfigPostgres;
    } else {
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
      config = { dialect: "sqlite", pollInterval: 0, connectionString: filename ?? ":memory:" };
    }

    const store = await SqlStore.create({
      ...options,
      passwordHashRounds: 1,
      config,
    });

    await store.deleteAllData();

    if (options.initialUsers) {
      for (const initialUser of options.initialUsers) {
        await store.createUser(initialUser);
      }
    }

    return store;
  });
});
