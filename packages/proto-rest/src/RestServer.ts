import { createLogger, parseBind, Store } from "@nexq/core";
import express, {
  Express,
  Request as ExpressRequest,
  Response as ExpressResponse,
  json,
  NextFunction,
  urlencoded,
} from "express";
import findRoot from "find-root";
import fs from "node:fs";
import http from "node:http";
import https, { ServerOptions as HttpsServerOptions } from "node:https";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as swaggerUI from "swagger-ui-express";
import { Swagger } from "tsoa";
import { applyAuthToExpress, applyAuthToSwaggerJson } from "./auth-utils.js";
import { RestConfig } from "./config.js";
import { iocContainer } from "./ioc.js";
import { RegisterRoutes } from "./routes/routes.js";
import { isHttpError } from "./utils.js";

const logger = createLogger("Rest:Server");

export class RestServer {
  private readonly app: Express;
  private httpServer?: http.Server;
  private httpsServer?: https.Server;

  public constructor(
    store: Store,
    private readonly config: RestConfig
  ) {
    iocContainer.setStore(store);

    const root = findRoot(fileURLToPath(import.meta.url));
    const swaggerJsonFilename = path.join(root, "src/routes/swagger.json");
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const swaggerJson: Swagger.Spec3 = JSON.parse(fs.readFileSync(swaggerJsonFilename, "utf8"));
    for (const auth of config.auth ?? []) {
      applyAuthToSwaggerJson(swaggerJson, auth);
    }

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
      res.redirect(301, "/docs");
    });
    this.app.use("/swagger.json", (_req, res) => {
      res.setHeader("Content-Type", "application/json");
      res.send(swaggerJson);
    });
    this.app.use(
      ["/openapi", "/docs", "/swagger"],
      swaggerUI.serve,
      swaggerUI.setup(null, {
        swaggerOptions: {
          url: "/swagger.json",
        },
      })
    );

    for (const auth of config.auth ?? []) {
      applyAuthToExpress(this.app, auth, store);
    }

    RegisterRoutes(this.app);

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
      for (const auth of this.config.auth ?? []) {
        logger.info(`  using auth: ${auth.type}`);
      }
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
