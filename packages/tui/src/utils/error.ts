export function getErrorMessage(err: unknown): string {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return `${err as any}`;
}
