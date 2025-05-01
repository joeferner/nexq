import https from "node:https";
import { Align, FlexDirection } from "yoga-layout";
import { Api, GetInfoResponse } from "../client/NexqClientApi.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { RouterElement } from "../render/RouterElement.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { ConfirmDialog } from "./ConfirmDialog.js";
import { Header } from "./Header.js";
import { MoveMessagesDialog } from "./MoveMessagesDialog.js";
import { QueueMessage } from "./QueueMessage.js";
import { QueueMessages } from "./QueueMessages.js";
import { Queues } from "./Queues.js";
import { StatusBar } from "./StatusBar.js";

const logger = createLogger("App");

export interface AppOptions {
  tuiVersion: string;
  url: string;
  rejectUnauthorized: boolean;
  ca?: Buffer;
  cert?: Buffer;
  key?: Buffer;
}

export class App extends Element {
  public readonly api: Api<unknown>;
  private readonly info: Promise<GetInfoResponse>;
  public readonly tuiVersion: string;
  public readonly refreshInterval = 5 * 1000;
  private readonly header: Header;
  private readonly queues: Queues;
  private readonly queueMessage: QueueMessage;
  private readonly queueMessages: QueueMessages;
  private readonly statusBar: StatusBar;
  public readonly confirmDialog: ConfirmDialog;
  public readonly moveMessagesDialog: MoveMessagesDialog;

  public constructor(document: Document, options: AppOptions) {
    super(document);

    this.tuiVersion = options.tuiVersion;

    let httpsAgent;
    if (options.cert || options.key || options.ca) {
      httpsAgent = new https.Agent({
        rejectUnauthorized: options.rejectUnauthorized,
        ca: options.ca,
        cert: options.cert,
        key: options.key,
      });
    }
    this.api = new Api({ baseURL: options.url, httpsAgent });

    this.header = new Header(document);
    this.queues = new Queues(document);
    this.queueMessage = new QueueMessage(document);
    this.queueMessages = new QueueMessages(document);
    this.confirmDialog = new ConfirmDialog(document);
    this.moveMessagesDialog = new MoveMessagesDialog(document);
    this.statusBar = new StatusBar(document);
    this.style.width = "100%";
    this.style.height = "100%";
    this.style.flexDirection = FlexDirection.Column;
    this.style.alignItems = Align.Stretch;
    this.appendChild(this.confirmDialog);
    this.appendChild(this.moveMessagesDialog);
    this.appendChild(this.header);

    const routerElement = new RouterElement(document, {
      routes: [
        {
          pathname: QueueMessage.PATH,
          element: this.queueMessage,
        },
        {
          pathname: QueueMessages.PATH,
          element: this.queueMessages,
        },
        {
          pathname: Queues.PATH,
          element: this.queues,
        },
      ],
    });
    routerElement.style.flexDirection = FlexDirection.Column;
    routerElement.style.alignItems = Align.Stretch;
    routerElement.style.flexGrow = 1;
    this.appendChild(routerElement);
    this.appendChild(this.statusBar);

    this.info = this.api.api.getInfo().then((i) => i.data);

    process.on("uncaughtException", (err) => {
      logger.error("uncaughtException", err);
      StatusBar.setStatus(this.document, "Uncaught Exception", err);
    });
  }

  public get nexqVersion(): Promise<string> {
    return this.info.then((i) => i.version);
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "escape")) {
      this.window.history.back();
      return;
    }

    return super.onKeyDown(event);
  }

  public static getApp(el: Element): App {
    const app = el.closest(App);
    if (!app) {
      throw new Error("can't find app");
    }
    return app;
  }
}
