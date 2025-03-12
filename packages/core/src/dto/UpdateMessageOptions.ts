export interface UpdateMessageOptions {
  priority?: number;
  attributes?: Record<string, string>;
  visibilityTimeoutMs?: number;
}
