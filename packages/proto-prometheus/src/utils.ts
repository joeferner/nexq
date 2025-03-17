import { HttpError } from "http-errors";

export function isHttpError(err: unknown): err is HttpError {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return "status" in (err as any) && "statusCode" in (err as any);
}
