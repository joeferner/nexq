import bcrypt from "bcryptjs";
import { v6 as uuidv7 } from "uuid";

export function createId(): string {
  return uuidv7();
}

export async function hashPassword(password: string, rounds: number): Promise<string> {
  const salt = await bcrypt.genSalt(rounds);
  return await bcrypt.hash(password, salt);
}

export async function verifyPassword(password: string, hash: string): Promise<boolean> {
  return await bcrypt.compare(password, hash);
}

export function parseBind(
  bind: string,
  defaultHostname: string,
  defaultPort: number
): { port: number; hostname: string } {
  const parts = bind.trim().split(":");
  if (parts.length === 2) {
    const port = Number(parts[1]);
    if (isNaN(port)) {
      throw new Error(`could not parse bind: "${bind}", port must be a number`);
    }
    return { hostname: parts[0], port };
  } else if (parts.length === 1) {
    const port = Number(parts[0]);
    if (isNaN(port)) {
      return { hostname: parts[0], port: defaultPort };
    } else {
      return { hostname: defaultHostname, port };
    }
  } else {
    throw new Error(`could not parse bind: "${bind}"`);
  }
}
