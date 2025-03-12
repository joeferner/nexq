import { AuthBasicConfig, AuthConfig, Store, verifyPassword } from "@nexq/core";
import express from "express";
import basicAuth, { AsyncAuthorizerCallback } from "express-basic-auth";
import { Swagger } from "tsoa";

export function applyAuthToSwaggerJson(swaggerJson: Swagger.Spec3, auth: AuthConfig): void {
  if (auth.type === "basic") {
    return applyAuthToSwaggerJsonBasic(swaggerJson, auth);
  }

  throw new Error(`unexpected auth type: ${auth.type}`);
}

function applyAuthToSwaggerJsonBasic(swaggerJson: Swagger.Spec3, _auth: AuthBasicConfig): void {
  swaggerJson.components.securitySchemes ??= {};
  swaggerJson.components.securitySchemes.basicAuth = {
    type: "http",
    scheme: "basic",
  };
  for (const path of Object.values(swaggerJson.paths)) {
    for (const operation of Object.values(path)) {
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const op: Swagger.Operation3 = operation;
      if (op.operationId) {
        op.security ??= [];
        op.security.push({ basicAuth: [] });
      }
    }
  }
}

export function applyAuthToExpress(app: express.Express, auth: AuthConfig, store: Store): void {
  if (auth.type === "basic") {
    return applyAuthToExpressBasic(app, auth, store);
  }

  throw new Error(`unexpected auth type: ${auth.type}`);
}

async function authorizer(username: string, password: string, store: Store): Promise<boolean> {
  const user = await store.getUserByUsername(username);
  if (!user || !user.passwordHash) {
    return false;
  }
  const verified = await verifyPassword(password, user.passwordHash);
  return verified;
}

function applyAuthToExpressBasic(app: express.Express, _auth: AuthBasicConfig, store: Store): void {
  app.use(
    basicAuth({
      authorizeAsync: true,
      authorizer: (username: string, password: string, callback: AsyncAuthorizerCallback) => {
        authorizer(username, password, store)
          .then((r) => callback(null, r))
          .catch((err) => callback(err));
      },
    })
  );
}
