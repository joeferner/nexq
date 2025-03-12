import { AuthConfig, HttpConfig, HttpsConfig } from "@nexq/core";

export interface RestConfig {
  https?: HttpsConfig;
  http?: HttpConfig;
  auth?: AuthConfig[];
}
