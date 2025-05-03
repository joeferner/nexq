import https from "node:https";
import { Align, FlexDirection, Overflow } from "yoga-layout";
import { Api, GetInfoResponse } from "../client/NexqClientApi.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { RouterElement } from "../render/RouterElement.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { Command } from "./Command.js";
import { ConfirmDialog } from "./ConfirmDialog.js";
import { Header } from "./Header.js";
import { MoveMessagesDialog } from "./MoveMessagesDialog.js";
import { QueueMessage } from "./QueueMessage.js";
import { QueueMessages } from "./QueueMessages.js";
import { Queues } from "./Queues.js";
import { StatusBar } from "./StatusBar.js";
import { Topics } from "./Topics.js";

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
  private readonly command: Command;
  private readonly queues: Queues;
  private readonly topics: Topics;
  private readonly queueMessage: QueueMessage;
  private readonly queueMessages: QueueMessages;
  private readonly statusBar: StatusBar;
  public readonly confirmDialog: ConfirmDialog;
  public readonly moveMessagesDialog: MoveMessagesDialog;
  private readonly routerElement: RouterElement;

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
    this.command = new Command(document);
    this.command.onCommand = this.handleCommand.bind(this);
    this.queues = new Queues(document);
    this.queueMessage = new QueueMessage(document);
    this.queueMessages = new QueueMessages(document);
    this.topics = new Topics(document);
    this.confirmDialog = new ConfirmDialog(document);
    this.moveMessagesDialog = new MoveMessagesDialog(document);
    this.statusBar = new StatusBar(document);
    this.style.width = "100%";
    this.style.height = "100%";
    this.style.flexDirection = FlexDirection.Column;
    this.style.alignItems = Align.Stretch;
    this.style.overflow = Overflow.Hidden;
    this.appendChild(this.confirmDialog);
    this.appendChild(this.moveMessagesDialog);
    this.appendChild(this.header);
    this.appendChild(this.command);

    this.routerElement = new RouterElement(document, {
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
        {
          pathname: Topics.PATH,
          element: this.topics,
        },
      ],
    });
    this.routerElement.style.flexDirection = FlexDirection.Column;
    this.routerElement.style.alignItems = Align.Stretch;
    this.routerElement.style.flexGrow = 1;
    this.routerElement.style.flexShrink = 1;
    this.routerElement.style.overflow = Overflow.Hidden;
    this.appendChild(this.routerElement);
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

    if (isInputMatch(event, ":")) {
      this.command.show();
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

  private handleCommand(value: string): boolean {
    if (value === "queues") {
      if (this.window.location.pathname === Queues.PATH) {
        StatusBar.setStatus(this.document, "Already on Queues");
        return false;
      }
      this.window.history.pushState({}, "", Queues.PATH);
      return true;
    }
    if (value === "topics") {
      if (this.window.location.pathname === Topics.PATH) {
        StatusBar.setStatus(this.document, "Already on Topics");
        return false;
      }
      this.window.history.pushState({}, "", Topics.PATH);
      return true;
    }
    StatusBar.setStatus(this.document, "Invalid command");
    return false;
  }
}
