import { RealTime, ReceivedMessage } from "@nexq/core";
import { CreateStoreOptions, runStoreTest } from "@nexq/test";
import fs from "node:fs";
import * as R from "radash";
import { describe, expect, test } from "vitest";
import { SqlStore, SqlStoreCreateConfig, SqlStoreCreateConfigPostgres } from "./SqlStore.js";

const testPostgres = process.env["TEST_POSTGRES"];
const QUEUE1_NAME = "queue1";

async function createStore(options: CreateStoreOptions, otherOptions?: { resetData: boolean }): Promise<SqlStore> {
  let config: SqlStoreCreateConfig;

  if (testPostgres) {
    config = {
      dialect: "postgres",
      pollInterval: "10m",
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

  if (otherOptions?.resetData !== false) {
    await store.deleteAllData();

    if (options.initialUsers) {
      for (const initialUser of options.initialUsers) {
        await store.createUser(initialUser);
      }
    }
  }

  return store;
}

describe("SqlStore", async () => {
  await runStoreTest(createStore);

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

  if (testPostgres) {
    test("message from other server causes immediate receive", async () => {
      const time = new RealTime();
      const store1 = await createStore({ time });
      const store2 = await createStore({ time }, { resetData: false });
      const messageBody = "test message";
      let receivedMessage: ReceivedMessage | undefined;
      let receivedMessageTime: Date | undefined;

      await store1.createQueue(QUEUE1_NAME);
      const receivePromise = store1
        .receiveMessage(QUEUE1_NAME, { visibilityTimeoutMs: 30 * 1000, waitTimeMs: 30 * 1000 })
        .then((m) => {
          receivedMessage = m;
          receivedMessageTime = time.getCurrentTime();
        });
      await R.sleep(10);
      await store2.sendMessage(QUEUE1_NAME, messageBody);
      const endTime = time.getCurrentTime();

      await receivePromise;
      expect(receivedMessage?.body).toBe(messageBody);
      expect(receivedMessageTime?.getTime()).toBeLessThan(endTime.getTime() + 1000);
    });
  }
});
