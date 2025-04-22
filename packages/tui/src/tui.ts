import * as R from "radash";
import { App, AppOptions } from "./components/App.js";
import { Window } from "./render/Window.js";

export async function start(options: AppOptions): Promise<void> {
  const window = new Window({ url: "http://nexq/queue" });

  const app = new App(window.document, options);
  window.document.appendChild(app);

  window.show();

  while (true) {
    await R.sleep(1000);
  }
}
