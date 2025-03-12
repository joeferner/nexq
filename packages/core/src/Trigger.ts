import { Time } from "./Time.js";

export class Trigger<T> {
  private resolve?: (value: T | "timeout") => void;
  private reject?: (reason?: Error) => void;

  public constructor(private readonly time: Time) {}

  public wait(ms: number): Promise<T | "timeout"> {
    if (this.resolve || this.reject) {
      throw new Error("already waiting");
    }

    return new Promise<T | "timeout">((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
      this.time.setTimeout(() => {
        this.resolve = undefined;
        this.reject = undefined;
        resolve("timeout");
      }, ms);
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
    }
  }
}
