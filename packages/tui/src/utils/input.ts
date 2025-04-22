import { KeyboardEvent } from "../render/KeyboardEvent.js";

export function isInputMatch(event: KeyboardEvent, shortcut: string): boolean {
  let text: string | undefined = event.key.toLocaleLowerCase();
  if (!text) {
    return false;
  }
  const keys = {
    shift: event.shiftKey,
    ctrl: event.ctrlKey,
    meta: event.metaKey,
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
    return false;
  }

  if (text !== undefined) {
    return false;
  }

  if (Object.values(keys).some((v) => v)) {
    return false;
  }

  return true;
}

export function inputToChar(event: KeyboardEvent): string | undefined {
  if (event.metaKey || event.ctrlKey) {
    return undefined;
  }

  if (event.key.length === 1) {
    let ch = event.key;
    if (event.shiftKey) {
      ch = ch.toUpperCase();
    }
    return ch;
  }

  if (event.key === "space") {
    return " ";
  }

  return undefined;
}
