/* eslint no-console: "off" */

import { render } from "ink";
import https from "node:https";
import React from "react";
import { Api } from "./client/NexqClientApi.js";
import { FullScreen } from "./components/FullScreen.js";
import { Main } from "./components/Main.js";
import { StateProvider } from "./StateContext.js";

export interface StartOptions {
  url: string;
  tuiVersion: string;
  rejectUnauthorized: boolean;
  ca?: Buffer;
  cert?: Buffer;
  key?: Buffer;
}

export async function start(options: StartOptions): Promise<void> {
  let httpsAgent;
  if (options.cert || options.key || options.ca) {
    httpsAgent = new https.Agent({
      rejectUnauthorized: options.rejectUnauthorized,
      ca: options.ca,
      cert: options.cert,
      key: options.key,
    });
  }
  const api = new Api({ baseURL: options.url, httpsAgent });

  render(
    <StateProvider tuiVersion={options.tuiVersion} api={api}>
      <FullScreen>
        <Main />
      </FullScreen>
    </StateProvider>
  );
}
