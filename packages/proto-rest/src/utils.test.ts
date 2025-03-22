import createHttpError from "http-errors";
import { describe, expect, test } from "vitest";
import { isHttpError } from "./utils.js";

describe("utils", () => {
  test("isHttpError", () => {
    expect(isHttpError(new Error())).toBeFalsy();
    expect(isHttpError(createHttpError.Conflict(`test`))).toBeTruthy();
  });
});
