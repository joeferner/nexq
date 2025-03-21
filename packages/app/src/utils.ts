import setValue from "set-value";

export function envSubstitution(str: string): string {
  return str.replaceAll(/\$\{env:(.*?)\}/g, (_s, arg): string => {
    return process.env[arg as string] ?? "";
  });
}

export function applyConfigOverrides(config: object, configOverrides?: string[]): void {
  for (const configOverride of configOverrides ?? []) {
    applyConfigOverride(config, configOverride);
  }
}

function applyConfigOverride(config: object, configOverride: string): void {
  let isQuoted = false;
  configOverride = configOverride.trim();

  let path: string;
  let value: string | number;
  if (configOverride.endsWith('"')) {
    isQuoted = true;
    const previousQuote = configOverride.substring(0, configOverride.length - 1).lastIndexOf('"');
    if (previousQuote < 0) {
      throw new Error(`invalid config override "${configOverride}", mismatched quotes`);
    }
    const quotedValue = configOverride.substring(previousQuote).trim();
    const left = configOverride.substring(0, previousQuote).trim();
    if (!left.endsWith("=")) {
      throw new Error(`invalid config override "${configOverride}", could not parse`);
    }
    path = left.substring(0, left.length - 1);
    value = quotedValue.substring(1, quotedValue.length - 1);
  } else {
    const i = configOverride.lastIndexOf("=");
    if (i < 0) {
      throw new Error(`invalid config override "${configOverride}", missing equals (=)`);
    }
    path = configOverride.substring(0, i);
    value = configOverride.substring(i + 1);
  }

  if (!isQuoted) {
    const n = Number(value);
    if (!isNaN(n)) {
      value = n;
    }
  }

  setValue(config, path, value);
}
