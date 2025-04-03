import { Key } from "ink";

export interface Input {
  t: Date;
  text: string;
  key: Key;
}

export function isInputMatch(input: Input, shortcut: string): boolean {
  let text: string | undefined = input.text.toLocaleLowerCase();
  const keys = { ...input.key };
  const shortcutParts = shortcut.split("-");
  for (const shortcutPart of shortcutParts) {
    if (shortcutPart === "shift" && keys.shift) {
      keys.shift = false;
      continue;
    }
    if (shortcutPart === "ctrl" && keys.ctrl) {
      keys.ctrl = false;
      continue;
    }
    if (shortcutPart === text) {
      text = undefined;
      continue;
    }
  }

  if (text !== undefined) {
    return false;
  }

  if (Object.values(keys).some((v) => v)) {
    return false;
  }

  return true;
}
