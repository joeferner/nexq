import { createLogger, parseBind, Store } from "@nexq/core";
import express, {
  Express,
  Request as ExpressRequest,
  Response as ExpressResponse,
  json,
  NextFunction,
  urlencoded,
} from "express";
import fs from "node:fs";
import http from "node:http";
import https, { ServerOptions as HttpsServerOptions } from "node:https";
import { PrometheusConfig } from "./config.js";
import { isHttpError } from "./utils.js";
import createHttpError from "http-errors";
import { serverMetrics } from "./serverMetrics.js";

const logger = createLogger("PrometheusServer");

export class PrometheusServer {
  private readonly app: Express;
  private httpServer?: http.Server;
  private httpsServer?: https.Server;

  public constructor(
    store: Store,
    private readonly config: PrometheusConfig
  ) {
    const metricsUrl = config.metricsUrl ?? "/metrics";

    this.app = express();
    this.app.use(
      urlencoded({
        extended: true,
      })
    );
    this.app.use(json());
    this.app.use((req, _resp, next) => {
      logger.debug(`request ${req.method}: ${req.path}`);
      next();
    });

    this.app.get("/", (_req, res) => {
      res.redirect(301, metricsUrl);
    });
    this.app.get(metricsUrl, async (_req, res, next) => {
      try {
        await serverMetrics(store, res);
      } catch (err) {
        if (isHttpError(err)) {
          return next(err);
        }
        logger.error("failed to serve metrics", err);
        return next(createHttpError.InternalServerError("failed to serve metrics"));
      }
    });

    this.app.use((err: unknown, _req: ExpressRequest, res: ExpressResponse, next: NextFunction) => {
      if (isHttpError(err)) {
        res.statusMessage = err.message;
        res.status(err.statusCode).end();
        return;
      }

      next();
    });
  }

  public async start(): Promise<void> {
    if (this.config.http) {
      const { port, hostname } = parseBind(this.config.http.bind, "0.0.0.0", 7887);
      // eslint-disable-next-line @typescript-eslint/no-misused-promises
      this.httpServer = http.createServer(this.app).listen(port, hostname);
      logger.info(`http server started http://${this.config.http.bind}`);
    }
    if (this.config.https) {
      const { port, hostname } = parseBind(this.config.https.bind, "0.0.0.0", 7888);
      const ca = await fs.promises.readFile(this.config.https.ca, "utf8");
      const cert = await fs.promises.readFile(this.config.https.cert, "utf8");
      const key = await fs.promises.readFile(this.config.https.key, "utf8");
      const httpsOptions: HttpsServerOptions = {
        ca,
        cert,
        key,
      };
      // eslint-disable-next-line @typescript-eslint/no-misused-promises
      this.httpsServer = https.createServer(httpsOptions, this.app).listen(port, hostname);
      logger.info(`https server started https://${this.config.https.bind}`);
    }
  }

  public shutdown(): void {
    if (this.httpServer) {
      this.httpServer.close();
      this.httpServer = undefined;
    }
    if (this.httpsServer) {
      this.httpsServer.close();
      this.httpsServer = undefined;
    }
  }
}
