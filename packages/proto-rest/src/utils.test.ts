import { describe, expect, test } from "vitest";
import { bufferFromBase64 } from "./utils.js";

describe("utils", () => {
  test("bufferFromBase64", () => {
    expect(bufferFromBase64("dGVzdA==")).toEqual(Buffer.from("test"));
  });
});
