import { cursorHide, cursorShow, enterAlternativeScreen, exitAlternativeScreen } from "ansi-escapes";
import readline, { Key } from "node:readline";
import * as R from "radash";
import { App } from "./components/App.js";
import { NexqState, NexqStateOptions, Screen } from "./NexqState.js";
import { Renderer } from "./render/Renderer.js";
import { isInputMatch } from "./utils/input.js";
import { createLogger } from "./utils/logger.js";

const logger = createLogger("tui");

export async function start(options: NexqStateOptions): Promise<void> {
  const state = new NexqState(options);
  await state.init();
  const app = new App(state);
  const renderer = new Renderer();
  let inAlternateScreen = false;

  const render = async (): Promise<void> => {
    renderer.render(app);
  };

  const tryExitAlternativeScreen = (): void => {
    if (inAlternateScreen) {
      process.stdin.setRawMode(false);
      process.stdout.write(exitAlternativeScreen);
      process.stdout.write(cursorShow);
      inAlternateScreen = false;
    }
  };

  process.on("SIGWINCH", () => {
    void render();
  });
  process.on("SIGINT", () => {
    tryExitAlternativeScreen();
    process.exit(0);
  });
  process.on("SIGQUIT", () => {
    tryExitAlternativeScreen();
  });
  process.on("SIGTERM", () => {
    tryExitAlternativeScreen();
  });
  process.on("uncaughtException", (err) => {
    logger.error("uncaughtException", err);
    state.setStatus("Uncaught Exception", err);
  });

  try {
    process.stdout.write(enterAlternativeScreen);
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdout.write(cursorHide);

    readline.emitKeypressEvents(process.stdin);
    // windows command line only gets escape key through data and now keypress
    process.stdin.on("data", (key) => {
      if (key.length === 1 && key[0] === 27) {
        state.handleKeyPress("1b", { name: "escape" });
      }
    });
    process.stdin.on("keypress", (chunk: string, key: Key | undefined) => {
      if (key && key.ctrl && key.name === "c") {
        tryExitAlternativeScreen();
        process.exit(0);
      }
      // handled in process.stdin.on("data", ...) to support windows
      if (isInputMatch(key, "escape")) {
        return;
      }
      state.handleKeyPress(chunk, key);
    });
    process.stdin.resume();
    inAlternateScreen = true;
    await render();

    let renderTimeout: NodeJS.Timeout | undefined;
    state.on("changed", () => {
      logger.debug("changed");
      if (renderTimeout) {
        clearTimeout(renderTimeout);
        renderTimeout = undefined;
      }
      renderTimeout = setTimeout(() => {
        renderTimeout = undefined;
        void render();
      });
    });
    setTimeout(() => {
      state.screen = Screen.Queues;
    });

    while (true) {
      await R.sleep(1000);
    }
  } catch (err) {
    tryExitAlternativeScreen();
    throw err;
  }
}
