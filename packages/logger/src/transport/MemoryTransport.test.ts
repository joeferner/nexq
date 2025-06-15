import { describe, expect, test } from "vitest";
import { Message } from "../formatter/Formatter.js";
import { PatternFormatter } from "../formatter/PatternFormatter.js";
import { MemoryTransport } from "./MemoryTransport.js";
import { LogLevel } from "../LoggerConfig.js";

describe("MemoryTransport", () => {
  const message: Message = {
    time: new Date(2025, 6, 12, 5, 42, 15, 32),
    level: LogLevel.Info,
    message: "test1",
    params: [],
  };
  const formatter = new PatternFormatter();

  test("log", () => {
    const transport = new MemoryTransport();
    transport.log({ ...message, message: "test1" }, formatter);
    transport.log({ ...message, message: "test2" }, formatter);
    expect(transport.messages.length).toBe(2);
    expect(transport.messages[0].message).toBe("test1");
    expect(transport.messages[1].message).toBe("test2");
  });

  test("maxMessages", () => {
    const transport = new MemoryTransport({ maxMessages: 2 });
    transport.log({ ...message, message: "test1" }, formatter);
    transport.log({ ...message, message: "test2" }, formatter);
    transport.log({ ...message, message: "test3" }, formatter);
    expect(transport.messages.length).toBe(2);
    expect(transport.messages[0].message).toBe("test2");
    expect(transport.messages[1].message).toBe("test3");
  });
});
