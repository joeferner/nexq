export interface HttpConfig {
  bind: string;
}

export interface HttpsConfig extends HttpConfig {
  ca: string;
  cert: string;
  key: string;
}

export type AuthConfig = AuthBasicConfig;

export interface AuthBasicConfig {
  type: "basic";
}
