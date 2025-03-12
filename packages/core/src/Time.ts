export interface Timeout {
  clear(): void;
}

export interface Time {
  getCurrentTime(): Date;
  setTimeout(fn: () => void, ms: number): Timeout;
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
}
