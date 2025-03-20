export function envSubstitution(str: string): string {
  return str.replaceAll(/\$\{env:(.*?)\}/g, (_s, arg): string => {
    return process.env[arg as string] ?? "";
  });
}
