export interface Timeout {
  clear(): void;
}

export interface Time {
  getCurrentTime(): Date;
  setTimeout(fn: () => void, ms: number, options?: { signal?: AbortSignal }): Timeout;
  setTimeoutUntil(fn: () => void, untilTime: Date, options?: { signal?: AbortSignal }): unknown;
}

export class RealTime implements Time {
  public getCurrentTime(): Date {
    return new Date();
  }

  public setTimeout(fn: () => void, ms: number, options?: { signal?: AbortSignal }): Timeout {
    const t = setTimeout(fn, ms);
    if (options?.signal) {
      options.signal.onabort = (): void => {
        clearTimeout(t);
        fn();
      };
    }
    return {
      clear: () => clearTimeout(t),
    };
  }

  public setTimeoutUntil(fn: () => void, untilTime: Date, options?: { signal?: AbortSignal }): Timeout {
    return this.setTimeout(fn, untilTime.getTime() - this.getCurrentTime().getTime(), options);
  }
}
