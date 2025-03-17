import { HttpConfig, HttpsConfig } from "@nexq/core";

export interface PrometheusConfig {
  https?: HttpsConfig;
  http?: HttpConfig;
  metricsUrl?: string;
}
