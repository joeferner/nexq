import { Time, Timeout } from "@nexq/core";
import * as R from "radash";

interface MockTimeout {
  timeoutTime: Date;
  fn: () => void;
  signal?: AbortSignal;
}

export class MockTime implements Time {
  private t = new Date();
  private timeouts: MockTimeout[] = [];

  public getCurrentTime(): Date {
    return this.t;
  }

  public setTimeout(fn: () => void, ms: number, options?: { signal?: AbortSignal }): Timeout {
    const timeoutTime = new Date(this.t.getTime() + ms);
    return this.setTimeoutUntil(fn, timeoutTime, options);
  }

  public setTimeoutUntil(fn: () => void, timeoutTime: Date, options?: { signal?: AbortSignal }): Timeout {
    if (options?.signal) {
      options.signal.onabort = (): void => {
        fn();
      };
    }

    if (this.getCurrentTime() >= timeoutTime) {
      if (options?.signal?.aborted) {
        // skip calling function
      } else {
        fn();
      }
      return {
        clear: (): void => {},
      };
    }

    const timeout: MockTimeout = {
      timeoutTime,
      fn,
      signal: options?.signal,
    };
    this.timeouts.push(timeout);
    return {
      clear: (): void => {
        this.timeouts = this.timeouts.filter((t) => t !== timeout);
      },
    };
  }

  public async advance(ms: number): Promise<void> {
    this.t = new Date(this.t.getTime() + ms);
    for (let i = this.timeouts.length - 1; i >= 0; i--) {
      const timeout = this.timeouts[i];
      if (this.t >= timeout.timeoutTime) {
        this.timeouts.splice(i, 1);
        if (timeout.signal?.aborted) {
          // skip calling function
        } else {
          timeout.fn();
        }
      }
    }
    await R.sleep(0);
  }
}
