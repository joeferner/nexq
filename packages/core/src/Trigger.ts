import { Time } from "./Time.js";

export class Trigger<T> {
  private resolve?: (value: T | "timeout") => void;
  private reject?: (reason?: Error) => void;
  private triggered?: { value: T };

  public constructor(private readonly time: Time) {}

  public wait(ms: number, options?: { signal?: AbortSignal }): Promise<T | "timeout"> {
    if (this.triggered !== undefined) {
      return Promise.resolve(this.triggered.value);
    }

    if (this.resolve || this.reject) {
      throw new Error("already waiting");
    }

    return new Promise<T | "timeout">((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
      this.time.setTimeout(
        () => {
          this.resolve = undefined;
          this.reject = undefined;
          resolve("timeout");
        },
        ms,
        options
      );
    });
  }

  public waitUntil(untilTime: Date, options?: { signal?: AbortSignal }): Promise<T | "timeout"> {
    if (this.triggered !== undefined) {
      return Promise.resolve(this.triggered.value);
    }

    if (this.resolve || this.reject) {
      throw new Error("already waiting");
    }

    return new Promise<T | "timeout">((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
      this.time.setTimeoutUntil(
        () => {
          this.resolve = undefined;
          this.reject = undefined;
          resolve("timeout");
        },
        untilTime,
        options
      );
    });
  }

  public trigger(t: T): void {
    if (this.resolve) {
      const resolve = this.resolve;
      this.resolve = undefined;
      this.reject = undefined;
      setTimeout(() => {
        resolve(t);
      }, 0);
    } else {
      this.triggered = { value: t };
    }
  }
}
