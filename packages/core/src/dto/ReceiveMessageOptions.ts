export interface ReceiveMessageOptions {
  visibilityTimeoutMs?: number;
  waitTimeMs?: number;
  abortSignal?: AbortSignal;
}
