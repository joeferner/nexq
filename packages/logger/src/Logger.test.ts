import { describe, expect, test } from "vitest";
import { Logger } from "./Logger.js";
import { MemoryTransport } from "./transport/MemoryTransport.js";
import { MessageContext } from "./MessageContext.js";
import { LogLevel } from "./LoggerConfig.js";

describe("Logger", () => {
  test("level", () => {
    const logger = new Logger();
    const memoryTransport = new MemoryTransport();

    logger.configure({
      level: LogLevel.Info,
      appenders: [{ transport: memoryTransport }],
    });

    logger.debug("testDebug");
    logger.info("testInfo");
    logger.info("testWarn");

    expect(memoryTransport.messages.length).toBe(2);
    expect(memoryTransport.messages[0].message).toBe("testInfo");
    expect(memoryTransport.messages[1].message).toBe("testWarn");
  });

  test("transport level", () => {
    const logger = new Logger();
    const memoryTransportDefault = new MemoryTransport();
    const memoryTransportInfo = new MemoryTransport();
    const memoryTransportWarn = new MemoryTransport();

    logger.configure({
      level: LogLevel.Debug,
      appenders: [
        { transport: memoryTransportDefault },
        { level: LogLevel.Info, transport: memoryTransportInfo },
        { level: LogLevel.Warn, transport: memoryTransportWarn },
      ],
    });

    logger.debug("testDebug");
    logger.info("testInfo");
    logger.warn("testWarn");

    expect(memoryTransportDefault.messages.length).toBe(3);
    expect(memoryTransportDefault.messages[0].message).toBe("testDebug");
    expect(memoryTransportDefault.messages[1].message).toBe("testInfo");
    expect(memoryTransportDefault.messages[2].message).toBe("testWarn");

    expect(memoryTransportInfo.messages.length).toBe(2);
    expect(memoryTransportInfo.messages[0].message).toBe("testInfo");
    expect(memoryTransportInfo.messages[1].message).toBe("testWarn");

    expect(memoryTransportWarn.messages.length).toBe(1);
    expect(memoryTransportWarn.messages[0].message).toBe("testWarn");
  });

  test("message context", () => {
    const logger = new Logger();
    const memoryTransport = new MemoryTransport();
    logger.configure({
      level: LogLevel.Debug,
      appenders: [
        { transport: memoryTransport },
      ],
    });

    const messageContext = new MessageContext({ value: 42 });
    logger.info('test message', 42, messageContext);

    expect(memoryTransport.messages[0].message).toBe('test message');
    expect(memoryTransport.messages[0].params).toStrictEqual([42]);
    expect(memoryTransport.messages[0].messageContext).toStrictEqual(messageContext);
  });
});
