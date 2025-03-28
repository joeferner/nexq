import * as grpc from "@grpc/grpc-js";
import { getErrorMessage, QueueInfo, QueueNotFoundError, Store } from "@nexq/core";
import { ExternalScalerMetadata, parseExternalScalerMetadata } from "./ExternalScalerMetadata.js";
import {
  ExternalScalerServer,
  GetMetricSpecResponse,
  GetMetricsRequest,
  GetMetricsResponse,
  IsActiveResponse,
  ScaledObjectRef,
} from "./generated/ExternalScaler.js";
import { InvalidArgumentError } from "./error/InvalidArgumentError.js";

export const MESSAGES_METRIC_NAME = "messages";

let _store: Store;

export class ExternalScalerImpl implements ExternalScalerServer {
  [name: string]: grpc.UntypedHandleCall;

  public constructor(store: Store) {
    _store = store;
  }

  public isActive(
    call: grpc.ServerUnaryCall<ScaledObjectRef, IsActiveResponse>,
    callback: grpc.sendUnaryData<IsActiveResponse>
  ): void {
    const run = async (): Promise<void> => {
      const metadata = parseExternalScalerMetadata(call.request.scalerMetadata);
      const queueInfo = await getQueueInfo(_store, metadata, callback);
      if (queueInfo === undefined) {
        return;
      }

      return callback(null, {
        result: queueInfo.numberOfMessages > metadata.activationThreshold,
      });
    };
    void run().catch((err) => handleError(err, callback));
  }

  public streamIsActive(_call: grpc.ServerWritableStream<ScaledObjectRef, IsActiveResponse>): void {
    throw new Error();
  }

  public getMetricSpec(
    call: grpc.ServerUnaryCall<ScaledObjectRef, GetMetricSpecResponse>,
    callback: grpc.sendUnaryData<GetMetricSpecResponse>
  ): void {
    const run = async (): Promise<void> => {
      const metadata = parseExternalScalerMetadata(call.request.scalerMetadata);

      return callback(null, {
        metricSpecs: [
          {
            metricName: MESSAGES_METRIC_NAME,
            targetSize: Math.ceil(metadata.threshold),
            targetSizeFloat: metadata.threshold,
          },
        ],
      });
    };
    void run().catch((err) => handleError(err, callback));
  }

  public getMetrics(
    call: grpc.ServerUnaryCall<GetMetricsRequest, GetMetricsResponse>,
    callback: grpc.sendUnaryData<GetMetricsResponse>
  ): void {
    const run = async (): Promise<void> => {
      if (!call.request.scaledObjectRef) {
        throw new InvalidArgumentError("metadata is required");
      }

      const metadata = parseExternalScalerMetadata(call.request.scaledObjectRef.scalerMetadata);
      const queueInfo = await getQueueInfo(_store, metadata, callback);
      if (queueInfo === undefined) {
        return;
      }

      return callback(null, {
        metricValues: [
          {
            metricName: MESSAGES_METRIC_NAME,
            metricValue: queueInfo.numberOfMessages,
            metricValueFloat: queueInfo.numberOfMessages,
          },
        ],
      });
    };
    void run().catch((err) => handleError(err, callback));
  }
}

async function getQueueInfo(
  store: Store,
  metadata: ExternalScalerMetadata,
  callback:
    | grpc.sendUnaryData<IsActiveResponse>
    | grpc.sendUnaryData<GetMetricSpecResponse>
    | grpc.sendUnaryData<GetMetricsResponse>
): Promise<QueueInfo | undefined> {
  if (!metadata.queueName) {
    callback({
      code: grpc.status.INVALID_ARGUMENT,
      details: '"queueName" must be specified',
    });
    return undefined;
  }

  try {
    return await store.getQueueInfo(metadata.queueName);
  } catch (err) {
    if (err instanceof QueueNotFoundError) {
      callback({
        code: grpc.status.INVALID_ARGUMENT,
        details: `could not find queue "${metadata.queueName}"`,
      });
      return undefined;
    } else {
      throw err;
    }
  }
}

function handleError(
  err: unknown,
  callback:
    | grpc.sendUnaryData<IsActiveResponse>
    | grpc.sendUnaryData<GetMetricSpecResponse>
    | grpc.sendUnaryData<GetMetricsResponse>
): void {
  if (err instanceof InvalidArgumentError) {
    return callback({
      code: grpc.status.INVALID_ARGUMENT,
      message: err.message,
    });
  }

  callback({
    code: grpc.status.INTERNAL,
    message: getErrorMessage(err),
  });
}
