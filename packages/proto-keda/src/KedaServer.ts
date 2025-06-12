import * as grpc from "@grpc/grpc-js";
import { getErrorMessage, Store } from "@nexq/core";
import { KedaConfig } from "./config.js";
import { ExternalScalerImpl } from "./ExternalScalerImpl.js";
import { ExternalScalerService } from "./generated/ExternalScaler.js";
import fs from "node:fs";
import { logger } from "@nexq/logger";

const log = logger.getLogger("Keda:Server");

export class KedaServer {
  private readonly server: grpc.Server;

  public constructor(
    store: Store,
    private readonly config: KedaConfig
  ) {
    this.server = new grpc.Server();
    this.server.addService(ExternalScalerService, new ExternalScalerImpl(store));
  }

  public async start(): Promise<void> {
    let credentials;
    let serverSecurityType;
    if (this.config.ca || this.config.cert || this.config.key) {
      if (!this.config.ca) {
        throw new Error("ca is required");
      }
      let rootCerts;
      try {
        rootCerts = await fs.promises.readFile(this.config.ca);
      } catch (err) {
        throw new Error(`Failed to read ca file "${this.config.ca}": ${getErrorMessage(err)}`);
      }

      if (!this.config.cert) {
        throw new Error("cert is required");
      }
      let cert_chain;
      try {
        cert_chain = await fs.promises.readFile(this.config.cert);
      } catch (err) {
        throw new Error(`Failed to read cert file "${this.config.cert}": ${getErrorMessage(err)}`);
      }

      if (!this.config.key) {
        throw new Error("key is required");
      }
      let private_key;
      try {
        private_key = await fs.promises.readFile(this.config.key);
      } catch (err) {
        throw new Error(`Failed to read key file "${this.config.key}": ${getErrorMessage(err)}`);
      }

      const keyCertPairs: grpc.KeyCertPair[] = [
        {
          cert_chain,
          private_key,
        },
      ];

      credentials = grpc.ServerCredentials.createSsl(
        rootCerts,
        keyCertPairs,
        this.config.checkClientCertificate ?? true
      );
      serverSecurityType = "ssl";
    } else {
      credentials = grpc.ServerCredentials.createInsecure();
      serverSecurityType = "unsecure";
    }

    await new Promise<void>((resolve, reject) => {
      this.server.bindAsync(this.config.bind, credentials, (err, _port) => {
        if (err) {
          return reject(err);
        }
        log.info(`${serverSecurityType} grpc server started on ${this.config.bind}`);
        resolve();
      });
    });
  }

  public async shutdown(): Promise<void> {
    this.server.forceShutdown();
  }
}
