import { Key } from "readline";

export function isInputMatch(key: Key | undefined, shortcut: string): boolean {
  if (!key) {
    return false;
  }
  let text: string | undefined = key.name?.toLocaleLowerCase();
  if (!text) {
    return false;
  }
  const keys = {
    shift: key.shift,
    ctrl: key.ctrl,
    meta: key.meta,
  };
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
