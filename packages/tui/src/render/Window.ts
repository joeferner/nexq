import { logger } from "@nexq/logger";
import { cursorHide, cursorShow, enterAlternativeScreen, exitAlternativeScreen } from "ansi-escapes";
import readline from "node:readline";
import { Key } from "readline";
import { Document } from "./Document.js";
import { Element, ElementEvents } from "./Element.js";
import { History, PopStateEvent, PushStateEvent } from "./History.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { EditableLocation, Location } from "./Location.js";
import { Renderer } from "./Renderer.js";

const log = logger.getLogger("window");

export interface WindowOptions {
  url?: string;
}

export interface WindowEvents extends ElementEvents {
  load: () => unknown;
  popstate: () => unknown;
  pushstate: () => unknown;
}

export class Window extends Element {
  public readonly location: Location;
  public readonly history = new History({
    getLocation: (): Location => {
      return this.location;
    },
    pushUrl: (url, event): void => {
      this.pushUrlFromHistory(url, event);
    },
    popUrl: (url, event): void => {
      this.popUrlFromHistory(url, event);
    },
  });
  private readonly renderer = new Renderer();
  private inAlternateScreen = false;
  private renderTimeout?: NodeJS.Timeout;
  private renderResolveFunctions: (() => unknown)[] = [];

  public constructor(options?: WindowOptions) {
    super(new Document());
    this.appendChild(this.document);

    process.on("SIGWINCH", () => {
      void this.refresh();
    });
    process.on("SIGINT", () => {
      this.tryExitAlternativeScreen();
      process.exit(0);
    });
    process.on("SIGQUIT", () => {
      this.tryExitAlternativeScreen();
    });
    process.on("SIGTERM", () => {
      this.tryExitAlternativeScreen();
    });

    const url = new URL(options?.url ?? "/");
    this.location = new Location(url);
  }

  public async refresh(): Promise<void> {
    if (!this.document.documentElement) {
      throw new Error("missing document element");
    }
    const elem = this.document.documentElement;

    if (this.renderTimeout) {
      clearTimeout(this.renderTimeout);
      this.renderTimeout = undefined;
    }

    return new Promise((resolve) => {
      this.renderResolveFunctions.push(resolve);
      this.renderTimeout = setTimeout(() => {
        this.renderTimeout = undefined;
        this.renderer.render(elem);
        for (const renderResolveFunction of this.renderResolveFunctions) {
          setTimeout(renderResolveFunction);
        }
        this.renderResolveFunctions.length = 0;
      });
    });
  }

  public override get window(): Window {
    return this;
  }

  public get innerHeight(): number {
    return process.stdout.rows ?? 40;
  }

  public get innerWidth(): number {
    return process.stdout.columns ?? 80;
  }

  public show(): void {
    process.stdout.write(enterAlternativeScreen);
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdout.write(cursorHide);

    readline.emitKeypressEvents(process.stdin);
    // windows command line only gets escape key through data and now keypress
    process.stdin.on("data", (key) => {
      try {
        if (key.length === 1 && key[0] === 27) {
          this.handleKeyDown(
            new KeyboardEvent({
              altKey: false,
              ctrlKey: false,
              metaKey: false,
              shiftKey: false,
              key: "Escape",
            })
          );
          void this.refresh();
        }
      } catch (err) {
        log.error("failed to process stdin.data", err);
      }
    });
    process.stdin.on("keypress", (chunk: string | undefined, key: Key | undefined) => {
      try {
        if (key && key.ctrl && key.name === "c") {
          this.tryExitAlternativeScreen();
          process.exit(0);
        }
        // handled in process.stdin.on("data", ...) to support windows
        if (key?.name?.toLocaleLowerCase() === "escape") {
          return;
        }
        if (key) {
          const ctrlKey = key.ctrl ?? false;
          const metaKey = key.meta ?? false;
          let translatedKey = key.name ?? "";

          if (chunk && !key.name && !ctrlKey && !metaKey && chunk.length === 1) {
            translatedKey = chunk;
          }

          this.handleKeyDown(
            new KeyboardEvent({
              altKey: false,
              ctrlKey,
              metaKey,
              shiftKey: key.shift ?? false,
              key: translatedKey,
            })
          );
          void this.refresh();
        }
      } catch (err) {
        log.error("failed to process stdin.keypress", err);
      }
    });
    process.stdin.resume();
    this.inAlternateScreen = true;
    void this.refresh();
    this.eventEmitter.emit("load");
  }

  private tryExitAlternativeScreen(): void {
    if (this.inAlternateScreen) {
      process.stdin.setRawMode(false);
      process.stdout.write(exitAlternativeScreen);
      process.stdout.write(cursorShow);
      this.inAlternateScreen = false;
    }
  }

  public override get isMounted(): boolean {
    return true;
  }

  private pushUrlFromHistory(url: URL, event: PushStateEvent): void {
    this.setUrl(url);
    this.eventEmitter.emit("pushstate", event);
  }

  private popUrlFromHistory(url: URL, event: PopStateEvent): void {
    this.setUrl(url);
    this.eventEmitter.emit("popstate", event);
  }

  private setUrl(url: URL): void {
    (this.location as unknown as EditableLocation).setUrl(url);
  }

  public addEventListener<T extends keyof WindowEvents>(event: T, listener: WindowEvents[T]): void {
    this.eventEmitter.addListener(event, listener);
  }

  public removeEventListener<T extends keyof WindowEvents>(event: T, listener: WindowEvents[T]): void {
    this.eventEmitter.removeListener(event, listener);
  }
}
