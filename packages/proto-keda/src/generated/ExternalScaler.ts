// Code generated by protoc-gen-ts_proto. DO NOT EDIT.
// versions:
//   protoc-gen-ts_proto  v2.7.0
//   protoc               v3.19.1
// source: ExternalScaler.proto

/* eslint-disable */
import { BinaryReader, BinaryWriter } from "@bufbuild/protobuf/wire";
import {
  type CallOptions,
  ChannelCredentials,
  Client,
  type ClientOptions,
  type ClientReadableStream,
  type ClientUnaryCall,
  type handleServerStreamingCall,
  type handleUnaryCall,
  makeGenericClientConstructor,
  Metadata,
  type ServiceError,
  type UntypedServiceImplementation,
} from "@grpc/grpc-js";

export const protobufPackage = "externalscaler";

export interface ScaledObjectRef {
  name: string;
  namespace: string;
  scalerMetadata: { [key: string]: string };
}

export interface ScaledObjectRef_ScalerMetadataEntry {
  key: string;
  value: string;
}

export interface IsActiveResponse {
  result: boolean;
}

export interface GetMetricSpecResponse {
  metricSpecs: MetricSpec[];
}

export interface MetricSpec {
  metricName: string;
  /** deprecated, use targetSizeFloat instead */
  targetSize: number;
  targetSizeFloat: number;
}

export interface GetMetricsRequest {
  scaledObjectRef: ScaledObjectRef | undefined;
  metricName: string;
}

export interface GetMetricsResponse {
  metricValues: MetricValue[];
}

export interface MetricValue {
  metricName: string;
  /** deprecated, use metricValueFloat instead */
  metricValue: number;
  metricValueFloat: number;
}

function createBaseScaledObjectRef(): ScaledObjectRef {
  return { name: "", namespace: "", scalerMetadata: {} };
}

export const ScaledObjectRef: MessageFns<ScaledObjectRef> = {
  encode(message: ScaledObjectRef, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.name !== "") {
      writer.uint32(10).string(message.name);
    }
    if (message.namespace !== "") {
      writer.uint32(18).string(message.namespace);
    }
    Object.entries(message.scalerMetadata).forEach(([key, value]) => {
      ScaledObjectRef_ScalerMetadataEntry.encode({ key: key as any, value }, writer.uint32(26).fork()).join();
    });
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): ScaledObjectRef {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseScaledObjectRef();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.name = reader.string();
          continue;
        }
        case 2: {
          if (tag !== 18) {
            break;
          }

          message.namespace = reader.string();
          continue;
        }
        case 3: {
          if (tag !== 26) {
            break;
          }

          const entry3 = ScaledObjectRef_ScalerMetadataEntry.decode(reader, reader.uint32());
          if (entry3.value !== undefined) {
            message.scalerMetadata[entry3.key] = entry3.value;
          }
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): ScaledObjectRef {
    return {
      name: isSet(object.name) ? globalThis.String(object.name) : "",
      namespace: isSet(object.namespace) ? globalThis.String(object.namespace) : "",
      scalerMetadata: isObject(object.scalerMetadata)
        ? Object.entries(object.scalerMetadata).reduce<{ [key: string]: string }>((acc, [key, value]) => {
            acc[key] = String(value);
            return acc;
          }, {})
        : {},
    };
  },

  toJSON(message: ScaledObjectRef): unknown {
    const obj: any = {};
    if (message.name !== "") {
      obj.name = message.name;
    }
    if (message.namespace !== "") {
      obj.namespace = message.namespace;
    }
    if (message.scalerMetadata) {
      const entries = Object.entries(message.scalerMetadata);
      if (entries.length > 0) {
        obj.scalerMetadata = {};
        entries.forEach(([k, v]) => {
          obj.scalerMetadata[k] = v;
        });
      }
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<ScaledObjectRef>, I>>(base?: I): ScaledObjectRef {
    return ScaledObjectRef.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<ScaledObjectRef>, I>>(object: I): ScaledObjectRef {
    const message = createBaseScaledObjectRef();
    message.name = object.name ?? "";
    message.namespace = object.namespace ?? "";
    message.scalerMetadata = Object.entries(object.scalerMetadata ?? {}).reduce<{ [key: string]: string }>(
      (acc, [key, value]) => {
        if (value !== undefined) {
          acc[key] = globalThis.String(value);
        }
        return acc;
      },
      {}
    );
    return message;
  },
};

function createBaseScaledObjectRef_ScalerMetadataEntry(): ScaledObjectRef_ScalerMetadataEntry {
  return { key: "", value: "" };
}

export const ScaledObjectRef_ScalerMetadataEntry: MessageFns<ScaledObjectRef_ScalerMetadataEntry> = {
  encode(message: ScaledObjectRef_ScalerMetadataEntry, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.key !== "") {
      writer.uint32(10).string(message.key);
    }
    if (message.value !== "") {
      writer.uint32(18).string(message.value);
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): ScaledObjectRef_ScalerMetadataEntry {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseScaledObjectRef_ScalerMetadataEntry();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.key = reader.string();
          continue;
        }
        case 2: {
          if (tag !== 18) {
            break;
          }

          message.value = reader.string();
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): ScaledObjectRef_ScalerMetadataEntry {
    return {
      key: isSet(object.key) ? globalThis.String(object.key) : "",
      value: isSet(object.value) ? globalThis.String(object.value) : "",
    };
  },

  toJSON(message: ScaledObjectRef_ScalerMetadataEntry): unknown {
    const obj: any = {};
    if (message.key !== "") {
      obj.key = message.key;
    }
    if (message.value !== "") {
      obj.value = message.value;
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<ScaledObjectRef_ScalerMetadataEntry>, I>>(
    base?: I
  ): ScaledObjectRef_ScalerMetadataEntry {
    return ScaledObjectRef_ScalerMetadataEntry.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<ScaledObjectRef_ScalerMetadataEntry>, I>>(
    object: I
  ): ScaledObjectRef_ScalerMetadataEntry {
    const message = createBaseScaledObjectRef_ScalerMetadataEntry();
    message.key = object.key ?? "";
    message.value = object.value ?? "";
    return message;
  },
};

function createBaseIsActiveResponse(): IsActiveResponse {
  return { result: false };
}

export const IsActiveResponse: MessageFns<IsActiveResponse> = {
  encode(message: IsActiveResponse, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.result !== false) {
      writer.uint32(8).bool(message.result);
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): IsActiveResponse {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseIsActiveResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 8) {
            break;
          }

          message.result = reader.bool();
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): IsActiveResponse {
    return { result: isSet(object.result) ? globalThis.Boolean(object.result) : false };
  },

  toJSON(message: IsActiveResponse): unknown {
    const obj: any = {};
    if (message.result !== false) {
      obj.result = message.result;
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<IsActiveResponse>, I>>(base?: I): IsActiveResponse {
    return IsActiveResponse.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<IsActiveResponse>, I>>(object: I): IsActiveResponse {
    const message = createBaseIsActiveResponse();
    message.result = object.result ?? false;
    return message;
  },
};

function createBaseGetMetricSpecResponse(): GetMetricSpecResponse {
  return { metricSpecs: [] };
}

export const GetMetricSpecResponse: MessageFns<GetMetricSpecResponse> = {
  encode(message: GetMetricSpecResponse, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    for (const v of message.metricSpecs) {
      MetricSpec.encode(v!, writer.uint32(10).fork()).join();
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): GetMetricSpecResponse {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetMetricSpecResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.metricSpecs.push(MetricSpec.decode(reader, reader.uint32()));
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): GetMetricSpecResponse {
    return {
      metricSpecs: globalThis.Array.isArray(object?.metricSpecs)
        ? object.metricSpecs.map((e: any) => MetricSpec.fromJSON(e))
        : [],
    };
  },

  toJSON(message: GetMetricSpecResponse): unknown {
    const obj: any = {};
    if (message.metricSpecs?.length) {
      obj.metricSpecs = message.metricSpecs.map((e) => MetricSpec.toJSON(e));
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<GetMetricSpecResponse>, I>>(base?: I): GetMetricSpecResponse {
    return GetMetricSpecResponse.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<GetMetricSpecResponse>, I>>(object: I): GetMetricSpecResponse {
    const message = createBaseGetMetricSpecResponse();
    message.metricSpecs = object.metricSpecs?.map((e) => MetricSpec.fromPartial(e)) || [];
    return message;
  },
};

function createBaseMetricSpec(): MetricSpec {
  return { metricName: "", targetSize: 0, targetSizeFloat: 0 };
}

export const MetricSpec: MessageFns<MetricSpec> = {
  encode(message: MetricSpec, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.metricName !== "") {
      writer.uint32(10).string(message.metricName);
    }
    if (message.targetSize !== 0) {
      writer.uint32(16).int64(message.targetSize);
    }
    if (message.targetSizeFloat !== 0) {
      writer.uint32(25).double(message.targetSizeFloat);
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): MetricSpec {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseMetricSpec();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.metricName = reader.string();
          continue;
        }
        case 2: {
          if (tag !== 16) {
            break;
          }

          message.targetSize = longToNumber(reader.int64());
          continue;
        }
        case 3: {
          if (tag !== 25) {
            break;
          }

          message.targetSizeFloat = reader.double();
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): MetricSpec {
    return {
      metricName: isSet(object.metricName) ? globalThis.String(object.metricName) : "",
      targetSize: isSet(object.targetSize) ? globalThis.Number(object.targetSize) : 0,
      targetSizeFloat: isSet(object.targetSizeFloat) ? globalThis.Number(object.targetSizeFloat) : 0,
    };
  },

  toJSON(message: MetricSpec): unknown {
    const obj: any = {};
    if (message.metricName !== "") {
      obj.metricName = message.metricName;
    }
    if (message.targetSize !== 0) {
      obj.targetSize = Math.round(message.targetSize);
    }
    if (message.targetSizeFloat !== 0) {
      obj.targetSizeFloat = message.targetSizeFloat;
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<MetricSpec>, I>>(base?: I): MetricSpec {
    return MetricSpec.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<MetricSpec>, I>>(object: I): MetricSpec {
    const message = createBaseMetricSpec();
    message.metricName = object.metricName ?? "";
    message.targetSize = object.targetSize ?? 0;
    message.targetSizeFloat = object.targetSizeFloat ?? 0;
    return message;
  },
};

function createBaseGetMetricsRequest(): GetMetricsRequest {
  return { scaledObjectRef: undefined, metricName: "" };
}

export const GetMetricsRequest: MessageFns<GetMetricsRequest> = {
  encode(message: GetMetricsRequest, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.scaledObjectRef !== undefined) {
      ScaledObjectRef.encode(message.scaledObjectRef, writer.uint32(10).fork()).join();
    }
    if (message.metricName !== "") {
      writer.uint32(18).string(message.metricName);
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): GetMetricsRequest {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetMetricsRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.scaledObjectRef = ScaledObjectRef.decode(reader, reader.uint32());
          continue;
        }
        case 2: {
          if (tag !== 18) {
            break;
          }

          message.metricName = reader.string();
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): GetMetricsRequest {
    return {
      scaledObjectRef: isSet(object.scaledObjectRef) ? ScaledObjectRef.fromJSON(object.scaledObjectRef) : undefined,
      metricName: isSet(object.metricName) ? globalThis.String(object.metricName) : "",
    };
  },

  toJSON(message: GetMetricsRequest): unknown {
    const obj: any = {};
    if (message.scaledObjectRef !== undefined) {
      obj.scaledObjectRef = ScaledObjectRef.toJSON(message.scaledObjectRef);
    }
    if (message.metricName !== "") {
      obj.metricName = message.metricName;
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<GetMetricsRequest>, I>>(base?: I): GetMetricsRequest {
    return GetMetricsRequest.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<GetMetricsRequest>, I>>(object: I): GetMetricsRequest {
    const message = createBaseGetMetricsRequest();
    message.scaledObjectRef =
      object.scaledObjectRef !== undefined && object.scaledObjectRef !== null
        ? ScaledObjectRef.fromPartial(object.scaledObjectRef)
        : undefined;
    message.metricName = object.metricName ?? "";
    return message;
  },
};

function createBaseGetMetricsResponse(): GetMetricsResponse {
  return { metricValues: [] };
}

export const GetMetricsResponse: MessageFns<GetMetricsResponse> = {
  encode(message: GetMetricsResponse, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    for (const v of message.metricValues) {
      MetricValue.encode(v!, writer.uint32(10).fork()).join();
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): GetMetricsResponse {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetMetricsResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.metricValues.push(MetricValue.decode(reader, reader.uint32()));
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): GetMetricsResponse {
    return {
      metricValues: globalThis.Array.isArray(object?.metricValues)
        ? object.metricValues.map((e: any) => MetricValue.fromJSON(e))
        : [],
    };
  },

  toJSON(message: GetMetricsResponse): unknown {
    const obj: any = {};
    if (message.metricValues?.length) {
      obj.metricValues = message.metricValues.map((e) => MetricValue.toJSON(e));
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<GetMetricsResponse>, I>>(base?: I): GetMetricsResponse {
    return GetMetricsResponse.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<GetMetricsResponse>, I>>(object: I): GetMetricsResponse {
    const message = createBaseGetMetricsResponse();
    message.metricValues = object.metricValues?.map((e) => MetricValue.fromPartial(e)) || [];
    return message;
  },
};

function createBaseMetricValue(): MetricValue {
  return { metricName: "", metricValue: 0, metricValueFloat: 0 };
}

export const MetricValue: MessageFns<MetricValue> = {
  encode(message: MetricValue, writer: BinaryWriter = new BinaryWriter()): BinaryWriter {
    if (message.metricName !== "") {
      writer.uint32(10).string(message.metricName);
    }
    if (message.metricValue !== 0) {
      writer.uint32(16).int64(message.metricValue);
    }
    if (message.metricValueFloat !== 0) {
      writer.uint32(25).double(message.metricValueFloat);
    }
    return writer;
  },

  decode(input: BinaryReader | Uint8Array, length?: number): MetricValue {
    const reader = input instanceof BinaryReader ? input : new BinaryReader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseMetricValue();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1: {
          if (tag !== 10) {
            break;
          }

          message.metricName = reader.string();
          continue;
        }
        case 2: {
          if (tag !== 16) {
            break;
          }

          message.metricValue = longToNumber(reader.int64());
          continue;
        }
        case 3: {
          if (tag !== 25) {
            break;
          }

          message.metricValueFloat = reader.double();
          continue;
        }
      }
      if ((tag & 7) === 4 || tag === 0) {
        break;
      }
      reader.skip(tag & 7);
    }
    return message;
  },

  fromJSON(object: any): MetricValue {
    return {
      metricName: isSet(object.metricName) ? globalThis.String(object.metricName) : "",
      metricValue: isSet(object.metricValue) ? globalThis.Number(object.metricValue) : 0,
      metricValueFloat: isSet(object.metricValueFloat) ? globalThis.Number(object.metricValueFloat) : 0,
    };
  },

  toJSON(message: MetricValue): unknown {
    const obj: any = {};
    if (message.metricName !== "") {
      obj.metricName = message.metricName;
    }
    if (message.metricValue !== 0) {
      obj.metricValue = Math.round(message.metricValue);
    }
    if (message.metricValueFloat !== 0) {
      obj.metricValueFloat = message.metricValueFloat;
    }
    return obj;
  },

  create<I extends Exact<DeepPartial<MetricValue>, I>>(base?: I): MetricValue {
    return MetricValue.fromPartial(base ?? ({} as any));
  },
  fromPartial<I extends Exact<DeepPartial<MetricValue>, I>>(object: I): MetricValue {
    const message = createBaseMetricValue();
    message.metricName = object.metricName ?? "";
    message.metricValue = object.metricValue ?? 0;
    message.metricValueFloat = object.metricValueFloat ?? 0;
    return message;
  },
};

export type ExternalScalerService = typeof ExternalScalerService;
export const ExternalScalerService = {
  isActive: {
    path: "/externalscaler.ExternalScaler/IsActive",
    requestStream: false,
    responseStream: false,
    requestSerialize: (value: ScaledObjectRef) => Buffer.from(ScaledObjectRef.encode(value).finish()),
    requestDeserialize: (value: Buffer) => ScaledObjectRef.decode(value),
    responseSerialize: (value: IsActiveResponse) => Buffer.from(IsActiveResponse.encode(value).finish()),
    responseDeserialize: (value: Buffer) => IsActiveResponse.decode(value),
  },
  streamIsActive: {
    path: "/externalscaler.ExternalScaler/StreamIsActive",
    requestStream: false,
    responseStream: true,
    requestSerialize: (value: ScaledObjectRef) => Buffer.from(ScaledObjectRef.encode(value).finish()),
    requestDeserialize: (value: Buffer) => ScaledObjectRef.decode(value),
    responseSerialize: (value: IsActiveResponse) => Buffer.from(IsActiveResponse.encode(value).finish()),
    responseDeserialize: (value: Buffer) => IsActiveResponse.decode(value),
  },
  getMetricSpec: {
    path: "/externalscaler.ExternalScaler/GetMetricSpec",
    requestStream: false,
    responseStream: false,
    requestSerialize: (value: ScaledObjectRef) => Buffer.from(ScaledObjectRef.encode(value).finish()),
    requestDeserialize: (value: Buffer) => ScaledObjectRef.decode(value),
    responseSerialize: (value: GetMetricSpecResponse) => Buffer.from(GetMetricSpecResponse.encode(value).finish()),
    responseDeserialize: (value: Buffer) => GetMetricSpecResponse.decode(value),
  },
  getMetrics: {
    path: "/externalscaler.ExternalScaler/GetMetrics",
    requestStream: false,
    responseStream: false,
    requestSerialize: (value: GetMetricsRequest) => Buffer.from(GetMetricsRequest.encode(value).finish()),
    requestDeserialize: (value: Buffer) => GetMetricsRequest.decode(value),
    responseSerialize: (value: GetMetricsResponse) => Buffer.from(GetMetricsResponse.encode(value).finish()),
    responseDeserialize: (value: Buffer) => GetMetricsResponse.decode(value),
  },
} as const;

export interface ExternalScalerServer extends UntypedServiceImplementation {
  isActive: handleUnaryCall<ScaledObjectRef, IsActiveResponse>;
  streamIsActive: handleServerStreamingCall<ScaledObjectRef, IsActiveResponse>;
  getMetricSpec: handleUnaryCall<ScaledObjectRef, GetMetricSpecResponse>;
  getMetrics: handleUnaryCall<GetMetricsRequest, GetMetricsResponse>;
}

export interface ExternalScalerClient extends Client {
  isActive(
    request: ScaledObjectRef,
    callback: (error: ServiceError | null, response: IsActiveResponse) => void
  ): ClientUnaryCall;
  isActive(
    request: ScaledObjectRef,
    metadata: Metadata,
    callback: (error: ServiceError | null, response: IsActiveResponse) => void
  ): ClientUnaryCall;
  isActive(
    request: ScaledObjectRef,
    metadata: Metadata,
    options: Partial<CallOptions>,
    callback: (error: ServiceError | null, response: IsActiveResponse) => void
  ): ClientUnaryCall;
  streamIsActive(request: ScaledObjectRef, options?: Partial<CallOptions>): ClientReadableStream<IsActiveResponse>;
  streamIsActive(
    request: ScaledObjectRef,
    metadata?: Metadata,
    options?: Partial<CallOptions>
  ): ClientReadableStream<IsActiveResponse>;
  getMetricSpec(
    request: ScaledObjectRef,
    callback: (error: ServiceError | null, response: GetMetricSpecResponse) => void
  ): ClientUnaryCall;
  getMetricSpec(
    request: ScaledObjectRef,
    metadata: Metadata,
    callback: (error: ServiceError | null, response: GetMetricSpecResponse) => void
  ): ClientUnaryCall;
  getMetricSpec(
    request: ScaledObjectRef,
    metadata: Metadata,
    options: Partial<CallOptions>,
    callback: (error: ServiceError | null, response: GetMetricSpecResponse) => void
  ): ClientUnaryCall;
  getMetrics(
    request: GetMetricsRequest,
    callback: (error: ServiceError | null, response: GetMetricsResponse) => void
  ): ClientUnaryCall;
  getMetrics(
    request: GetMetricsRequest,
    metadata: Metadata,
    callback: (error: ServiceError | null, response: GetMetricsResponse) => void
  ): ClientUnaryCall;
  getMetrics(
    request: GetMetricsRequest,
    metadata: Metadata,
    options: Partial<CallOptions>,
    callback: (error: ServiceError | null, response: GetMetricsResponse) => void
  ): ClientUnaryCall;
}

export const ExternalScalerClient = makeGenericClientConstructor(
  ExternalScalerService,
  "externalscaler.ExternalScaler"
) as unknown as {
  new (address: string, credentials: ChannelCredentials, options?: Partial<ClientOptions>): ExternalScalerClient;
  service: typeof ExternalScalerService;
  serviceName: string;
};

type Builtin = Date | Function | Uint8Array | string | number | boolean | undefined;

export type DeepPartial<T> = T extends Builtin
  ? T
  : T extends globalThis.Array<infer U>
    ? globalThis.Array<DeepPartial<U>>
    : T extends ReadonlyArray<infer U>
      ? ReadonlyArray<DeepPartial<U>>
      : T extends {}
        ? { [K in keyof T]?: DeepPartial<T[K]> }
        : Partial<T>;

type KeysOfUnion<T> = T extends T ? keyof T : never;
export type Exact<P, I extends P> = P extends Builtin
  ? P
  : P & { [K in keyof P]: Exact<P[K], I[K]> } & { [K in Exclude<keyof I, KeysOfUnion<P>>]: never };

function longToNumber(int64: { toString(): string }): number {
  const num = globalThis.Number(int64.toString());
  if (num > globalThis.Number.MAX_SAFE_INTEGER) {
    throw new globalThis.Error("Value is larger than Number.MAX_SAFE_INTEGER");
  }
  if (num < globalThis.Number.MIN_SAFE_INTEGER) {
    throw new globalThis.Error("Value is smaller than Number.MIN_SAFE_INTEGER");
  }
  return num;
}

function isObject(value: any): boolean {
  return typeof value === "object" && value !== null;
}

function isSet(value: any): boolean {
  return value !== null && value !== undefined;
}

export interface MessageFns<T> {
  encode(message: T, writer?: BinaryWriter): BinaryWriter;
  decode(input: BinaryReader | Uint8Array, length?: number): T;
  fromJSON(object: any): T;
  toJSON(message: T): unknown;
  create<I extends Exact<DeepPartial<T>, I>>(base?: I): T;
  fromPartial<I extends Exact<DeepPartial<T>, I>>(object: I): T;
}
