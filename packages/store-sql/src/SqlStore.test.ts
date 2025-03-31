import { CreateStoreOptions, runStoreTest } from "@nexq/test";
import fs from "node:fs";
import { describe, expect, test } from "vitest";
import { SqlStore, SqlStoreCreateConfig, SqlStoreCreateConfigPostgres } from "./SqlStore.js";

describe("SqlStore", async () => {
  await runStoreTest(async (options: CreateStoreOptions) => {
    let config: SqlStoreCreateConfig;

    if (process.env["TEST_POSTGRES"]) {
      config = {
        dialect: "postgres",
        pollInterval: "30s",
        connectionString: "postgres://nexq-postgres/nexq",
        options: {
          user: "nexq",
          password: "password",
        },
      } satisfies SqlStoreCreateConfigPostgres;
    } else {
      let filename = expect.getState().currentTestName?.replaceAll(/[>:/\s]+/g, "_");
      if (filename) {
        filename = `test-results/${filename}.sqlite`;
        try {
          fs.unlinkSync(filename);
        } catch (_err) {
          // ok
        }
        fs.mkdirSync("test-results", { recursive: true });
      }
      config = { dialect: "sqlite", pollInterval: "30s", connectionString: filename ?? ":memory:" };
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

  test("maskPasswordInConnectionString", () => {
    expect(SqlStore.maskPasswordInConnectionString("postgresql://")).toBe("postgresql://");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://localhost")).toBe("postgresql://localhost");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://localhost:5432")).toBe("postgresql://localhost:5432");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://localhost/mydb")).toBe("postgresql://localhost/mydb");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://user@localhost")).toBe("postgresql://user@localhost");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://user:secret@localhost")).toBe(
      "postgresql://user:***@localhost"
    );
    expect(
      SqlStore.maskPasswordInConnectionString(
        "postgresql://other@localhost/otherdb?connect_timeout=10&application_name=myapp"
      )
    ).toBe("postgresql://other@localhost/otherdb?connect_timeout=10&application_name=myapp");
    expect(SqlStore.maskPasswordInConnectionString("postgresql://localhost/mydb?user=other&password=secret")).toBe(
      "postgresql://localhost/mydb?user=other&password=***"
    );
    expect(SqlStore.maskPasswordInConnectionString("Data Source=mydb.db;Version=3;Password=myPassword;")).toBe(
      "Data Source=mydb.db;Version=3;Password=***;"
    );
  });
});
