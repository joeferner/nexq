import https from "node:https";
import { Api } from "./client/NexqClientApi.js";

export interface NexqApiOptions {
    url: string;
    rejectUnauthorized: boolean;
    ca?: Buffer;
    cert?: Buffer;
    key?: Buffer;
}

export class NexqApi {
    private readonly api: Api<unknown>;

    public constructor(options: NexqApiOptions) {
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
}