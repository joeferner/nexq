import EventEmitter from "node:events";
import https from "node:https";
import { Key } from "readline";
import { Api, GetInfoResponse } from "./client/NexqClientApi.js";
import { ConfirmDialog } from "./components/ConfirmDialog.js";
import { MoveMessagesDialog } from "./components/MoveMessagesDialog.js";
import { Queues } from "./components/Queues.js";
import { getErrorMessage } from "./utils/error.js";

export interface NexqStateOptions {
  tuiVersion: string;
  url: string;
  rejectUnauthorized: boolean;
  ca?: Buffer;
  cert?: Buffer;
  key?: Buffer;
}

export enum Screen {
  None = "None",
  Queues = "Queues",
}

export interface StateEvents {
  changed: () => unknown;
  keypress: (chunk: string, key: Key | undefined) => unknown;
  screenChange: (newScreen: Screen) => unknown;
}

export class NexqState extends EventEmitter {
  public readonly api: Api<unknown>;
  private info!: GetInfoResponse;
  private _screen: Screen = Screen.None;
  private focusStack = [Queues.ID];
  public readonly confirmDialog: ConfirmDialog;
  public readonly moveMessagesDialog: MoveMessagesDialog;
  public readonly logoColor = "#fca321";
  public readonly headerNameColor = "#fca321";
  public readonly headerValueColor = "#ffffff";
  public readonly borderColor = "#97cdf5";
  public readonly titleColor = "#74fbfc";
  public readonly helpHotkeyColor = "#448ef7";
  public readonly helpNameColor = "#80807f";
  public readonly tableViewTextColor = "#96ccf5";
  public readonly dialogBorderColor = "#97cdf5";
  public readonly inputBoxFocusColor = "#ffffff";
  public readonly inputBoxFocusBgColor = "#00332D";
  public readonly tuiVersion: string;
  public readonly refreshInterval = 5 * 1000;
  public readonly statusTimeout = 3 * 1000;
  private _status = "";

  public constructor(options: NexqStateOptions) {
    super();

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

    this.confirmDialog = new ConfirmDialog(this);
    this.moveMessagesDialog = new MoveMessagesDialog(this);
  }

  public async init(): Promise<void> {
    this.info = (await this.api.api.getInfo()).data;
  }

  public get nexqVersion(): string {
    return this.info.version;
  }

  public get screen(): Screen {
    return this._screen;
  }

  public get status(): string {
    return this._status;
  }

  public get focus(): string {
    return this.focusStack[this.focusStack.length - 1];
  }

  public set focus(focus: string) {
    this.focusStack[this.focusStack.length - 1] = focus;
  }

  public pushFocus(focus: string): void {
    this.focusStack.push(focus);
  }

  public popFocus(): void {
    this.focusStack.pop();
  }

  public set screen(screen: Screen) {
    this._screen = screen;
    this.emit("screenChange", screen);
  }

  public handleKeyPress(chunk: string, key: Key | undefined): void {
    this.emit("keypress", chunk, key);
  }

  public setStatus(message: string, err?: unknown): void {
    let statusMessage = message;
    if (err) {
      statusMessage += ` ${getErrorMessage(err)}`;
    }
    if (statusMessage !== this._status) {
      this._status = statusMessage;
      this.emit("changed");
    }
  }

  public override on<T extends keyof StateEvents>(event: T, listener: StateEvents[T]): this {
    return super.on(event, listener);
  }

  public override emit<T extends keyof StateEvents>(event: T, ...args: Parameters<StateEvents[T]>): boolean {
    return super.emit(event, ...args);
  }
}
