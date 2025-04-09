import { enterAlternativeScreen, exitAlternativeScreen } from "ansi-escapes";
import { App } from "./components/App.js";
import { NexqApi, NexqApiOptions } from "./NexqApi.js";
import { Renderer } from "./render/Renderer.js";
import { State } from "./State.js";
import * as R from "radash";
import { logToFile } from "./utils/log.js";

export interface StartOptions extends NexqApiOptions {
  tuiVersion: string;
}

export async function start(options: StartOptions): Promise<void> {
  const api = new NexqApi(options);
  const state = new State();
  const app = new App(api);
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

  process.stdout.write(enterAlternativeScreen);
  process.stdin.setRawMode(true);
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
  void render();

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
}
