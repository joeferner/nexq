import { Time } from "@nexq/core";
import { Timeout } from "@nexq/core/build/Time.js";

interface MockTimeout {
  timeoutTime: Date;
  fn: () => void;
}

export class MockTime implements Time {
  private t = new Date();
  private timeouts: MockTimeout[] = [];

  public getCurrentTime(): Date {
    return this.t;
  }

  public setTimeout(fn: () => void, ms: number): Timeout {
    const timeoutTime = new Date(this.t.getTime() + ms);
    const timeout: MockTimeout = {
      timeoutTime: timeoutTime,
      fn,
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
        timeout.fn();
      }
    }
  }
}
