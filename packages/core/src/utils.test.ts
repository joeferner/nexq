import { describe, expect, test } from "vitest";
import { createId, hashPassword, parseBind, verifyPassword } from "./utils.js";

describe("utils", async () => {
  test("createId", () => {
    expect(createId()).toMatch(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/);
  });

  test("password", async () => {
    const hash = await hashPassword("test-password", 1);
    expect(await verifyPassword("test-password", hash)).toBeTruthy();
  });

  describe("parseBind", () => {
    test("good", () => {
      expect(parseBind("0.0.0.0:123", "z", 42)).toEqual({ hostname: "0.0.0.0", port: 123 });
      expect(parseBind("0.0.0.0", "z", 42)).toEqual({ hostname: "0.0.0.0", port: 42 });
      expect(parseBind("42", "1.2.3.4", 5)).toEqual({ hostname: "1.2.3.4", port: 42 });
    });

    test("bar", () => {
      expect(() => parseBind("0.0.0.0:123:4", "z", 42)).toThrowError(/could not parse.*/);
      expect(() => parseBind("0.0.0.0:123a", "z", 42)).toThrowError(/could not parse.*/);
    });
  });
});
