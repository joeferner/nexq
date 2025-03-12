export interface SqlMigration {
  version: number;
  name: string;
  applied_at: string | Date;
}
