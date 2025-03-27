export interface KedaConfig {
  bind: string;
  ca?: string;
  cert?: string;
  key?: string;
  checkClientCertificate?: boolean;
}
