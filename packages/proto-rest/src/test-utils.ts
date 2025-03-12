import { expect } from "vitest";
import { isHttpError } from "./utils.js";

export async function expectHttpError(
  fn: () => Promise<unknown>,
  expectedStatusCode: number,
  statusMessage?: string | RegExp
): Promise<void> {
  try {
    await fn();
    throw new Error(`expected HttpError with status code ${expectedStatusCode}`);
  } catch (err) {
    if (isHttpError(err)) {
      expect(err.statusCode).toBe(expectedStatusCode);
      if (statusMessage) {
        expect(err.message).toMatch(statusMessage);
      }
    } else {
      throw new Error(`expected HttpError with status code ${expectedStatusCode}`, { cause: err });
    }
  }
}
