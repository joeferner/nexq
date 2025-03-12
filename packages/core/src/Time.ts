export interface Timeout {
  clear(): void;
}

export interface Time {
  getCurrentTime(): Date;
  setTimeout(fn: () => void, ms: number): Timeout;
  setTimeoutUntil(fn: () => void, untilTime: Date): unknown;
}

export class RealTime implements Time {
  public getCurrentTime(): Date {
    return new Date();
  }

  public setTimeout(fn: () => void, ms: number): Timeout {
    const t = setTimeout(fn, ms);
    return {
      clear: () => clearTimeout(t),
    };
  }

  public setTimeoutUntil(fn: () => void, untilTime: Date): Timeout {
    return this.setTimeout(fn, untilTime.getTime() - this.getCurrentTime().getTime());
  }
}
