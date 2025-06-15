import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, test } from "vitest";
import { PatternFormatter } from "../formatter/PatternFormatter.js";
import { FileTransport } from "./FileTransport.js";
import { LogLevel } from "../LoggerConfig.js";

describe("FileTransport", () => {
  const tmpdir = os.tmpdir();
  const formatter = new PatternFormatter();

  test("write to file", () => {
    const tempFile = path.join(tmpdir, "nexq-logger-FileTransport.log");
    fs.rmSync(tempFile, { force: true });
    try {
      const transport = new FileTransport({ filename: tempFile });
      transport.log(
        {
          time: new Date(2025, 6, 12, 5, 42, 15, 32),
          level: LogLevel.Info,
          message: "test1",
          params: ["param1", 42],
        },
        formatter
      );
      transport.log(
        {
          time: new Date(2025, 6, 12, 5, 42, 16, 32),
          level: LogLevel.Info,
          message: "test2",
          params: [],
        },
        formatter
      );

      const lines = fs.readFileSync(tempFile, "utf8").split("\n");
      expect(lines.length).toBe(3);
      expect(lines[0]).toBe("2025-07-12T05:42:15.032Z: info: test1 param1 42");
      expect(lines[1]).toBe("2025-07-12T05:42:16.032Z: info: test2");
      expect(lines[2]).toBe("");
    } finally {
      fs.rmSync(tempFile, { force: true });
    }
  });
});
