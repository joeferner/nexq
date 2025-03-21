import { HttpError } from "http-errors";
import createError from "http-errors";

const BASE64_REGEX = /^([0-9a-zA-Z+/]{4})*(([0-9a-zA-Z+/]{2}==)|([0-9a-zA-Z+/]{3}=))?$/;

export function isHttpError(err: unknown): err is HttpError {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return "status" in (err as any) && "statusCode" in (err as any);
}

export function bufferFromBase64(base64Str: string): Buffer {
  if (!BASE64_REGEX.test(base64Str)) {
    throw new createError.BadRequest("invalid base64");
  }
  return Buffer.from(base64Str, "base64");
}
