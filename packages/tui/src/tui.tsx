/* eslint no-console: "off" */

import { render } from "ink";
import React from "react";
import { ApiContext } from "./ApiContext.js";
import { FullScreen } from "./components/FullScreen.js";
import { Main } from "./components/Main.js";
import { Api } from "./client/NexqClientApi.js";
import https from "node:https";

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
    <ApiContext.Provider value={api}>
      <FullScreen>
        <Main tuiVersion={options.tuiVersion} />
      </FullScreen>
    </ApiContext.Provider>
  );
}
