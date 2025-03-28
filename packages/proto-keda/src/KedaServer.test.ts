import * as grpc from "@grpc/grpc-js";
import { MemoryStore } from "@nexq/store-memory";
import { MockTime } from "@nexq/test";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { KedaConfig } from "./config.js";
import { MESSAGES_METRIC_NAME } from "./ExternalScalerImpl.js";
import {
  ExternalScalerClient,
  GetMetricSpecResponse,
  GetMetricsResponse,
  IsActiveResponse,
  ScaledObjectRef,
} from "./generated/ExternalScaler.js";
import { KedaServer } from "./KedaServer.js";

const QUEUE1_NAME = "queue1";
const PORT = 9999;
const DEFAULT_OPTIONS = {
  name: "test",
  namespace: "test-ns",
  scalerMetadata: {
    queueName: QUEUE1_NAME,
    activationThreshold: "0",
    threshold: "5",
  },
};

describe("KedaServer", () => {
  let store!: MemoryStore;
  let client!: ExternalScalerClient;
  let server!: KedaServer;

  beforeEach(async () => {
    store = await MemoryStore.create({
      time: new MockTime(),
      config: {
        pollInterval: "10s",
      },
    });
    await store.createQueue(QUEUE1_NAME);

    const kedaServerConfig: KedaConfig = {
      bind: `0.0.0.0:${PORT}`,
    };

    server = new KedaServer(store, kedaServerConfig);
    await server.start();

    client = new ExternalScalerClient(`localhost:${PORT}`, grpc.credentials.createInsecure());
  });

  afterEach(async () => {
    client.close();
    await server.shutdown();
    await store.shutdown();
  });

  test("isActive", async () => {
    const resultBefore = await callIsActive(client);
    expect(resultBefore.result).toBeFalsy();

    await store.sendMessage(QUEUE1_NAME, "test");

    const resultAfterSend = await callIsActive(client);
    expect(resultAfterSend.result).toBeTruthy();

    const message = await store.receiveMessage(QUEUE1_NAME);

    const resultAfterReceive = await callIsActive(client);
    expect(resultAfterReceive.result).toBeTruthy();

    await store.deleteMessage(QUEUE1_NAME, message!.id, message!.receiptHandle);

    const resultAfterDelete = await callIsActive(client);
    expect(resultAfterDelete.result).toBeFalsy();
  });

  test("getMetricSpec", async () => {
    const result = await callGetMetricSpec(client);
    expect(result.metricSpecs.length).toBe(1);
    expect(result.metricSpecs[0]).toEqual({
      metricName: MESSAGES_METRIC_NAME,
      targetSize: 5,
      targetSizeFloat: 5,
    });
  });

  test("getMetrics", async () => {
    const result = await callGetMetrics(client, MESSAGES_METRIC_NAME);
    expect(result.metricValues.length).toBe(1);
    expect(result.metricValues[0]).toEqual({
      metricName: MESSAGES_METRIC_NAME,
      metricValue: 0,
      metricValueFloat: 0,
    });
  });
});

function callIsActive(client: ExternalScalerClient): Promise<IsActiveResponse> {
  return new Promise((resolve, reject) =>
    client.isActive(ScaledObjectRef.create(DEFAULT_OPTIONS), (err, result) => {
      if (err) {
        return reject(err);
      }
      resolve(result);
    })
  );
}

function callGetMetricSpec(client: ExternalScalerClient): Promise<GetMetricSpecResponse> {
  return new Promise((resolve, reject) =>
    client.getMetricSpec(ScaledObjectRef.create(DEFAULT_OPTIONS), (err, result) => {
      if (err) {
        return reject(err);
      }
      resolve(result);
    })
  );
}

function callGetMetrics(client: ExternalScalerClient, metricName: string): Promise<GetMetricsResponse> {
  return new Promise((resolve, reject) =>
    client.getMetrics({ metricName, scaledObjectRef: ScaledObjectRef.create(DEFAULT_OPTIONS) }, (err, result) => {
      if (err) {
        return reject(err);
      }
      resolve(result);
    })
  );
}
