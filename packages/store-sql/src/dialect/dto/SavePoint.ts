export interface SavePoint {
  release(): Promise<void>;
  rollback(): Promise<void>;
}
