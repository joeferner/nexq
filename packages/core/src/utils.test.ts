import { describe, expect, test } from "vitest";
import {
  createId,
  hashPassword,
  parseBind,
  parseOptionalBytesSize,
  parseOptionalDurationIntoMs,
  verifyPassword,
} from "./utils.js";

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

  describe("parseOptionalDurationIntoMs", () => {
    test("good", () => {
      expect(parseOptionalDurationIntoMs("2us")).toBe(0.002);
      expect(parseOptionalDurationIntoMs("3ms")).toBe(3);
      expect(parseOptionalDurationIntoMs("3000ms")).toBe(3000);
      expect(parseOptionalDurationIntoMs("4s")).toBe(4 * 1000);
      expect(parseOptionalDurationIntoMs("5m")).toBe(5 * 60 * 1000);
      expect(parseOptionalDurationIntoMs("6h")).toBe(6 * 60 * 60 * 1000);
      expect(parseOptionalDurationIntoMs("7d")).toBe(7 * 24 * 60 * 60 * 1000);
      expect(parseOptionalDurationIntoMs("4,000s")).toBe(4000 * 1000);
    });

    test("good ISO duration", () => {
      expect(parseOptionalDurationIntoMs("PT5S")).toBe(5 * 1000);
      expect(parseOptionalDurationIntoMs("PT0.056S")).toBe(56);
      expect(parseOptionalDurationIntoMs("P2D")).toBe(2 * 24 * 60 * 60 * 1000);
    });

    test("bad", () => {
      expect(() => parseOptionalDurationIntoMs("2z")).toThrowError(/could not parse.*/);
      expect(() => parseOptionalDurationIntoMs("")).toThrowError(/could not parse.*/);
      expect(() => parseOptionalDurationIntoMs("ams")).toThrowError(/could not parse.*/);
    });
  });

  describe("parseOptionalBytesSize", () => {
    test("good", () => {
      expect(parseOptionalBytesSize("2")).toBe(2);
      expect(parseOptionalBytesSize("2bytes")).toBe(2);
      expect(parseOptionalBytesSize("3kb")).toBe(3 * 1024);
      expect(parseOptionalBytesSize("3000kb")).toBe(3000 * 1024);
      expect(parseOptionalBytesSize("4mb")).toBe(4 * 1024 * 1024);
      expect(parseOptionalBytesSize("5gb")).toBe(5 * 1024 * 1024 * 1024);
      expect(parseOptionalBytesSize("6tb")).toBe(6 * 1024 * 1024 * 1024 * 1024);
    });

    test("bad", () => {
      expect(() => parseOptionalBytesSize("2z")).toThrowError(/could not parse.*/);
      expect(() => parseOptionalBytesSize("")).toThrowError(/could not parse.*/);
      expect(() => parseOptionalBytesSize("ams")).toThrowError(/could not parse.*/);
    });
  });
});
