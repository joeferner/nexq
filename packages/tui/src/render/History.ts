import { Location } from "./Location.js";

export interface HistoryOptions {
  getLocation: () => Location;
  pushUrl: (url: URL) => unknown;
  popUrl: (url: URL) => unknown;
}

export class History {
  private stack: string[] = [];

  public constructor(private readonly options: HistoryOptions) {}

  public pushState(_state: object, _unused: string, url?: string): void {
    const currentHref = this.options.getLocation().href;
    this.stack.push(currentHref);

    if (url && url.startsWith("/")) {
      const newUrl = new URL(url, currentHref);
      this.options.pushUrl(newUrl);
      return;
    }

    throw new Error("unhandled push state");
  }

  public back(): void {
    if (this.stack.length > 0) {
      const newUrl = new URL(this.stack[this.stack.length - 1]);
      this.stack.splice(this.stack.length - 1, 1);
      this.options.popUrl(newUrl);
    }
  }
}
