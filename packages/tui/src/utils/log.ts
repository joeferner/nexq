import fs from "node:fs";

export function logToFile(str: string): void {
  fs.appendFileSync("/tmp/nexq-tui.log", `${new Date().toISOString()}: ${str}\n`);
}
