import { isAxiosError } from "axios";

export function getErrorMessage(err: unknown): string {
  if (isAxiosError(err)) {
    if (err.response?.statusText) {
      return err.response.statusText;
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return `${err as any}`;
}
