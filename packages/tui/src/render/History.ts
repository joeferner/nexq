import { Location } from "./Location.js";

export interface HistoryOptions {
  getLocation: () => Location;
  pushUrl: (url: URL, event: PushStateEvent) => unknown;
  popUrl: (url: URL, event: PopStateEvent) => unknown;
}

export interface PushStateEvent {
  state: object;
}

export interface PopStateEvent {
  state: object;
}

interface StackItem {
  url: string;
  state: object;
}

export class History {
  private stack: StackItem[] = [];

  public constructor(private readonly options: HistoryOptions) {}

  public pushState(state: object, _unused: string, url?: string): void {
    const currentHref = this.options.getLocation().href;
    this.stack.push({ url: currentHref, state });

    if (url && url.startsWith("/")) {
      const newUrl = new URL(url, currentHref);
      this.options.pushUrl(newUrl, { state });
      return;
    }

    throw new Error("unhandled push state");
  }

  public get state(): object {
    return this.stack[this.stack.length - 1]?.state ?? {};
  }

  public popState(): void {
    if (this.stack.length > 0) {
      const item = this.stack[this.stack.length - 1];
      const newUrl = new URL(item.url);
      this.stack.splice(this.stack.length - 1, 1);
      this.options.popUrl(newUrl, { state: item.state });
    }
  }

  public back(): void {
    if (this.stack.length > 0) {
      const item = this.stack[this.stack.length - 1];
      const newUrl = new URL(item.url);
      // TODO keep item on stack and just move stack pointer
      this.stack.splice(this.stack.length - 1, 1);
      this.options.popUrl(newUrl, { state: item.state });
    }
  }
}
