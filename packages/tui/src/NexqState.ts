import https from "node:https";
import { Api, GetInfoResponse } from "./client/NexqClientApi.js";
import EventEmitter from "node:events";
import { Key } from "readline";

export interface NexqStateOptions {
    tuiVersion: string;
    url: string;
    rejectUnauthorized: boolean;
    ca?: Buffer;
    cert?: Buffer;
    key?: Buffer;
}

export interface StateEvents {
    'changed': () => unknown;
    'keypress': (chunk: string, key: Key | undefined) => unknown;
}

export class NexqState extends EventEmitter {
    private readonly api: Api<unknown>;
    private info!: GetInfoResponse;
    public readonly logoColor = '#fca321';
    public readonly headerNameColor = "#fca321";
    public readonly headerValueColor = '#ffffff';
    public readonly tuiVersion: string;

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
    }

    public async init(): Promise<void> {
        this.info = (await this.api.api.getInfo()).data;
    }

    public get nexqVersion(): string {
        return this.info.version;
    }

    public handleKeyPress(chunk: string, key: Key | undefined): void {
        this.emit('keypress', chunk, key);
    }

    public override on<T extends keyof StateEvents>(event: T, listener: StateEvents[T]): this {
        return super.on(event, listener);
    }

    public override emit<T extends keyof StateEvents>(event: T, ...args: Parameters<StateEvents[T]>): boolean {
        return super.emit(event, ...args);
    }
}