import { HttpError } from "http-errors";

const DURATION_SUFFIX: Record<string, number> = {
  us: 0.001,
  ms: 1,
  s: 1000,
  m: 60 * 1000,
  h: 60 * 60 * 1000,
  d: 24 * 60 * 60 * 1000,
};

export function parseOptionalDurationIntoMs(str: string | undefined): number | undefined {
  if (str === undefined) {
    return undefined;
  }
  str = str.replace(/,/g, "").trim();

  let multiplier: number | undefined = undefined;
  let numberPart: string | undefined = undefined;
  for (const suffix of Object.keys(DURATION_SUFFIX)) {
    if (str.endsWith(suffix)) {
      numberPart = str.substring(0, str.length - suffix.length);
      multiplier = DURATION_SUFFIX[suffix];
      break;
    }
  }

  if (multiplier === undefined || numberPart === undefined) {
    throw new DurationParseError(str);
  }

  const n = Number(numberPart);
  if (isNaN(n)) {
    throw new DurationParseError(str);
  }
  return n * multiplier;
}

export class DurationParseError extends Error {
  public constructor(public readonly str: string) {
    super(`could not parse duration "${str}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}

const BYTES_SIZE_SUFFIX: Record<string, number> = {
  bytes: 1,
  kb: 1024,
  mb: 1024 * 1024,
  gb: 1024 * 1024 * 1024,
  tb: 1024 * 1024 * 1024 * 1024,
};

export function parseOptionalBytesSize(str: number | string | undefined): number | undefined {
  if (str === undefined) {
    return undefined;
  }
  str = `${str}`.replace(/,/g, "").trim();
  if (str.length === 0) {
    throw new SizeParseError(str);
  }

  let multiplier: number | undefined = undefined;
  let numberPart: string | undefined = undefined;
  for (const suffix of Object.keys(BYTES_SIZE_SUFFIX)) {
    if (str.endsWith(suffix)) {
      numberPart = str.substring(0, str.length - suffix.length);
      multiplier = BYTES_SIZE_SUFFIX[suffix];
      break;
    }
  }

  if (multiplier === undefined || numberPart === undefined) {
    multiplier = 1;
    numberPart = str;
  }

  const n = Number(numberPart);
  if (isNaN(n)) {
    throw new SizeParseError(str);
  }
  return n * multiplier;
}

export class SizeParseError extends Error {
  public constructor(public readonly str: string) {
    super(`could not parse size "${str}"`);
    this.name = this.constructor.name;
    Error.captureStackTrace(this, this.constructor);
  }
}

export function isHttpError(err: unknown): err is HttpError {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return "status" in (err as any) && "statusCode" in (err as any);
}
