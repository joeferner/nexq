import { createLogger } from "@nexq/core";
import findRoot from "find-root";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Controller, Get, Route, SuccessResponse, Tags } from "tsoa";
import { GetInfoResponse } from "../dto/GetInfoResponse.js";

const logger = createLogger("Rest:ApiV1QueueController");

export interface User {
  userId: number;
  name?: string;
}

@Tags("info")
@Route("api/v1")
export class ApiV1InfoController extends Controller {
  private info?: Promise<GetInfoResponse>;

  public constructor() {
    super();
    void this.loadInfo();
  }

  private async loadInfo(): Promise<GetInfoResponse> {
    if (this.info) {
      return this.info;
    }

    const _loadInfo = async (): Promise<GetInfoResponse> => {
      const root = findRoot(fileURLToPath(import.meta.url));
      const packageJsonFilename = path.join(root, "package.json");
      // eslint-disable-next-line  @typescript-eslint/no-unsafe-assignment
      const packageJson: { version: string } = JSON.parse(await fs.promises.readFile(packageJsonFilename, "utf8"));
      return {
        version: packageJson.version,
      };
    };

    this.info = new Promise((resolve, reject) => {
      _loadInfo().then(resolve).catch(reject);
    });

    return this.info;
  }

  /**
   * gets info about the running NexQ
   */
  @Get("info")
  @SuccessResponse("200", "Info")
  public async getInfo(): Promise<GetInfoResponse> {
    try {
      const info = await this.loadInfo();
      return info;
    } catch (err) {
      logger.error(`failed to get info`, err);
      throw err;
    }
  }
}
