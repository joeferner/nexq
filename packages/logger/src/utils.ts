export function isString(value: unknown): value is string {
  return typeof value === "string";
}

export function toString(value: unknown): string {
  if (isString(value)) {
    return value;
  }

  throw new Error("invalid string value");
}

export function isBoolean(value: unknown): value is boolean {
  return typeof value === "boolean" || value === true || value === false;
}

export function toBoolean(value: unknown): boolean {
  if (isBoolean(value)) {
    return value;
  }

  if (isString(value)) {
    const str = value.toLocaleLowerCase().trim();
    if (str === "true") {
      return true;
    } else if (str === "false") {
      return false;
    }
  }

  throw new Error("invalid boolean value");
}

export function isNumber(value: unknown): value is number {
  return typeof value === "number";
}

export function toNumber(value: unknown): number {
  if (isNumber(value)) {
    return value;
  }

  if (isString(value)) {
    return parseFloat(value);
  }

  throw new Error("Could not convert to number");
}

export function toInteger(value: unknown): number {
  const num = toNumber(value);
  if (Number.isInteger(num)) {
    return num;
  }
  throw new Error("value is not an integer");
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function anyToString(value: any): string {
  if (value instanceof Error) {
    return value.stack ?? value.message;
  }
  if (typeof value === "object" && value !== null) {
    try {
      return JSON.stringify(value, null, 2);
    } catch (_err) {
      return String(value);
    }
  }
  return String(value);
}
