import { enterAlternativeScreen, exitAlternativeScreen } from "ansi-escapes";
import * as R from "radash";
import { App } from "./components/App.js";
import { NexqState, NexqStateOptions } from "./NexqState.js";
import { Renderer } from "./render/Renderer.js";
import { logToFile } from "./utils/log.js";

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
      inAlternateScreen = false;
    }
  };

  process.on('SIGWINCH', () => {
    void render();
  });
  process.on('SIGINT', () => {
    tryExitAlternativeScreen();
    process.exit(0);
  });
  process.on('SIGQUIT', () => {
    tryExitAlternativeScreen();
  });
  process.on('SIGTERM', () => {
    tryExitAlternativeScreen();
  });

  try {
    process.stdout.write(enterAlternativeScreen);
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdin.on('data', (key) => {
      const str = key.toString();
      if (str === '\x03') { // Ctrl+C
        tryExitAlternativeScreen();
        process.exit(0);
      } else {
        logToFile(`key: ${str}`);
      }
    });
    inAlternateScreen = true;
    await render();

    let renderTimeout: NodeJS.Timeout | undefined;
    state.on('changed', () => {
      if (renderTimeout) {
        clearTimeout(renderTimeout);
        renderTimeout = undefined;
      }
      renderTimeout = setTimeout(() => {
        renderTimeout = undefined;
        void render();
      });
    });

    while (true) {
      await R.sleep(1000);
    }
  } catch (err) {
    tryExitAlternativeScreen();
    throw err;
  }
}
