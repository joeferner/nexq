import { expect } from "vitest";
import { isHttpError } from "./utils.js";
import express from "express";
import EventEmitter from "events";

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

export class MockRequestSocket extends EventEmitter {}

export class MockRequest extends EventEmitter {
  public readonly socket = new MockRequestSocket();

  public close(): void {
    this.emit("close");
    this.socket.emit("close");
  }
}

export function createMockRequest(): express.Request & MockRequest {
  return new MockRequest() as express.Request & MockRequest;
}
