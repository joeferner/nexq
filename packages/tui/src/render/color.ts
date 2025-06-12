import { logger } from "@nexq/logger";
import { Color as AnsiSequenceColor, parseAnsiSequences } from "ansi-sequence-parser";
import * as ansis from "ansis";
import { getNamedColors, NamedColor } from "named-css-colors";

const log = logger.getLogger("Color");
const namedColors = getNamedColors();

export function fgColor(color: string): ansis.Ansis {
  const hexColor = toHexColor(color);
  return ansis.hex(hexColor);
}

export function bgColor(color: string): ansis.Ansis {
  const hexColor = toHexColor(color);
  return ansis.bgHex(hexColor);
}

function toHexColor(color: string): string {
  if (color.startsWith("#")) {
    return color;
  }

  const namedColor = namedColors[color as NamedColor];
  if (namedColor) {
    return namedColor;
  }

  log.warn(`unknown color: ${color}`);
  return color;
}

export function findClosestNamedColor(color: string, options?: { excludes?: string[] }): string {
  const hexColor = toHexColor(color);
  let minColor = "";
  let minDistance = Number.MAX_SAFE_INTEGER;
  const rgb = parseHexColorToRgb(hexColor);
  for (const name of Object.keys(namedColors)) {
    if (options?.excludes?.includes(name)) {
      continue;
    }
    const v = parseHexColorToRgb(namedColors[name as NamedColor]);
    const distance = colorDistance(rgb, v);
    if (distance < minDistance) {
      minDistance = distance;
      minColor = name;
    }
  }
  return minColor;
}

function parseHexColorToRgb(hex: string): [number, number, number] {
  if (!hex.startsWith("#")) {
    throw new Error(`expected hex color but found "${hex}"`);
  }
  const r = parseInt(hex.substring(1, 3), 16);
  const g = parseInt(hex.substring(3, 5), 16);
  const b = parseInt(hex.substring(5, 7), 16);
  return [r, g, b];
}

function colorDistance(rgb1: [number, number, number], rgb2: [number, number, number]): number {
  const rMean = (rgb1[0] + rgb2[0]) / 2;
  const r = rgb1[0] - rgb2[0];
  const g = rgb1[1] - rgb2[1];
  const b = rgb1[2] - rgb2[2];
  return Math.sqrt((((512 + rMean) * r * r) >> 8) + 4 * g * g + (((767 - rMean) * b * b) >> 8));
}

export function ansiColorToColorString(color: AnsiSequenceColor): string {
  const hex = (n: number): string => {
    return n.toString(16).padStart(2, "0");
  };

  if (color.type === "rgb") {
    return `#${hex(color.rgb[0])}${hex(color.rgb[1])}${hex(color.rgb[2])}`;
  } else if (color.type === "named") {
    const name = color.name.toLocaleLowerCase().replace("bright", "light");
    const namedColor = namedColors[name as NamedColor];
    if (namedColor) {
      return namedColor;
    }
    log.warn(`color not supported "${JSON.stringify(color.type)}"`);
    return "#ffffff";
  } else {
    log.warn(`color not supported "${JSON.stringify(color.type)}"`);
    return "#ffffff";
  }
}

export function ansiLength(str: string): number {
  return parseAnsiSequences(str).reduce((p, v) => p + v.value.length, 0);
}
