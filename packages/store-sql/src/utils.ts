import * as R from "radash";

export function parseDate(value: string | Date): Date {
  if (R.isDate(value)) {
    return value;
  }
  return new Date(value);
}

export function parseOptionalDate(value: string | Date | null): Date | undefined {
  if (R.isDate(value)) {
    return value;
  }
  if (value === null) {
    return undefined;
  }
  return parseDate(value);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function clearRecord<K extends keyof any, T>(record: Record<K, T>): void {
  for (const key in record) {
    delete record[key];
  }
}
