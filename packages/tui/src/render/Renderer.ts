import { cursorHide, cursorShow, cursorTo } from "ansi-escapes";
import { createAnsiSequenceParser } from "ansi-sequence-parser";
import * as ansis from "ansis";
import Yoga, { Direction } from "yoga-layout";
import { createLogger } from "../utils/logger.js";
import { ansiColorToColorString, bgColor, fgColor } from "./color.js";
import { Element } from "./Element.js";
import { Geometry, geometryFromYogaNode, rectIntersection } from "./Geometry.js";
import { BoxRenderItem, findRenderItem, RenderItem, TextRenderItem } from "./RenderItem.js";
import { BorderStyle } from "./Style.js";

const logger = createLogger("Renderer");

interface Character {
  value: string;
  color: string;
  bgColor?: string;
  inverse: boolean;
  zIndex: number;
}

interface BorderCharacters {
  nw: string;
  n: string;
  ne: string;
  e: string;
  se: string;
  s: string;
  sw: string;
  w: string;
}

const BORDERS: Record<BorderStyle, BorderCharacters> = {
  none: {
    nw: "",
    n: "",
    ne: "",
    e: "",
    se: "",
    s: "",
    sw: "",
    w: "",
  },
  solid: {
    nw: "┌",
    n: "─",
    ne: "┐",
    e: "│",
    se: "┘",
    s: "─",
    sw: "└",
    w: "│",
  },
};

function isCharacterEqual(ch1: Character, ch2: Character | undefined): boolean {
  if (ch2 === undefined) {
    return false;
  }
  return (
    ch1.value === ch2.value && ch1.bgColor === ch2.bgColor && ch1.color === ch2.color && ch1.inverse === ch2.inverse
  );
}

export class Renderer {
  private _width = 0;
  private _height = 0;
  private previousWidth = 0;
  private previousHeight = 0;
  private buffers: Character[][][] = [];
  private bufferIndex = 0;

  public get width(): number {
    return this._width;
  }

  public get height(): number {
    return this._height;
  }

  public render(element: Element): void {
    const startTime = Date.now();
    this._width = process.stdout.columns ?? 80;
    this._height = process.stdout.rows ?? 40;

    const root: BoxRenderItem = {
      type: "box",
      children: [],
      zIndex: 0,
      geometry: {
        top: 0,
        left: 0,
        height: 0,
        width: 0,
      },
    };
    const rootYogaNode = Yoga.Node.create();
    try {
      rootYogaNode.setWidth(this.width);
      rootYogaNode.setHeight(this.height);
      element.populateLayout(rootYogaNode);
      rootYogaNode.calculateLayout(undefined, undefined, Direction.LTR);
      root.geometry = geometryFromYogaNode(rootYogaNode);
      element.render({
        parent: root,
        root,
      });
    } finally {
      rootYogaNode.freeRecursive();
    }

    const secondaryBufferIndex = (this.bufferIndex + 1) % 2;
    if (this._width !== this.previousWidth || this._height !== this.previousHeight) {
      this.buffers[0] = createBuffer(this._width, this._height);
      this.buffers[1] = createBuffer(this._width, this._height);
      clearBuffer(this.buffers[secondaryBufferIndex], ""); // force rerender of all characters
    }

    clearBuffer(this.buffers[this.bufferIndex]);
    renderItemGeometryToAbsolute(root, root.geometry, 0);
    renderItemToBuffer(this.buffers[this.bufferIndex], root, root.geometry);

    renderBuffer(this.buffers[this.bufferIndex], this.buffers[secondaryBufferIndex]);

    const cursorRenderItem = findRenderItem(root, (r) => r.type === "cursor");
    if (cursorRenderItem) {
      process.stdout.write(cursorShow);
      process.stdout.write(cursorTo(cursorRenderItem.geometry.left, cursorRenderItem.geometry.top));
    } else {
      process.stdout.write(cursorHide);
    }

    this.previousWidth = this._width;
    this.previousHeight = this._height;
    this.bufferIndex = secondaryBufferIndex;

    const endTime = Date.now();
    const deltaT = endTime - startTime;
    logger.debug(`render ${deltaT}ms, ${((1 / deltaT) * 1000).toFixed(2)}fps`);
  }
}

function clearBuffer(buffer: Character[][], ch?: string): void {
  for (let y = 0; y < buffer.length; y++) {
    const row = buffer[y];
    for (let x = 0; x < row.length; x++) {
      row[x].value = ch ?? " ";
      row[x].inverse = false;
      row[x].zIndex = Number.MIN_SAFE_INTEGER;
    }
  }
}

function renderBuffer(buffer: Character[][], lastBuffer?: Character[][]): void {
  const ansiCache: Record<string, ansis.Ansis> = {};

  const createColorKey = (ch: Character): string => {
    return `${ch.color}${ch.bgColor}${ch.inverse ? "t" : "f"}`;
  };

  const getAnsi = (key: string, ch: Character): ansis.Ansis => {
    const existing = ansiCache[key];
    if (existing) {
      return existing;
    }
    let ansi = fgColor(ch.color);
    if (ch.bgColor) {
      ansi = bgColor(ch.bgColor);
    }
    if (ch.inverse) {
      ansi = ansi.inverse;
    }
    ansiCache[key] = ansi;
    return ansi;
  };

  let nextY = -1;
  let nextX = -1;
  let lastKey = "";
  let lastAnsi: ansis.Ansis | undefined = undefined;

  for (let y = 0; y < buffer.length; y++) {
    const row = buffer[y];
    const lastRow = lastBuffer?.[y];
    for (let x = 0; x < row.length; x++) {
      if (y === buffer.length - 1 && x === row.length - 1) {
        break;
      }

      const ch = row[x];
      if (!isCharacterEqual(ch, lastRow?.[x])) {
        if (y !== nextY || x !== nextX) {
          process.stdout.write(cursorTo(x, y));
          nextY = y;
        }

        const key = createColorKey(ch);
        if (key !== lastKey) {
          if (lastAnsi) {
            process.stdout.write(lastAnsi.close);
          }
          const ansi = getAnsi(key, ch);
          process.stdout.write(ansi.open);
          lastAnsi = ansi;
          lastKey = key;
        }
        process.stdout.write(ch.value);
        nextX = x + 1;
      }
    }
    nextX = -1;
  }

  if (lastAnsi) {
    process.stdout.write(lastAnsi.close);
  }
}

function renderItemGeometryToAbsolute(renderItem: RenderItem, container: Geometry, zIndex: number): void {
  renderItem.geometry.top = container.top + renderItem.geometry.top;
  renderItem.geometry.left = container.left + renderItem.geometry.left;
  renderItem.zIndex += zIndex;
  if ("children" in renderItem) {
    for (const child of renderItem.children) {
      renderItemGeometryToAbsolute(child, renderItem.geometry, renderItem.zIndex);
    }
  }
}

function renderItemToBuffer(buffer: Character[][], renderItem: RenderItem, container: Geometry): void {
  const type = renderItem.type;

  if (renderItem.name?.endsWith("-title")) {
    renderItem.name = "" + renderItem.name;
  }

  const newContainer = rectIntersection(renderItem.geometry, container);
  if (!newContainer) {
    return;
  }
  container = newContainer;

  if (type === "cursor") {
    // do nothing
  } else if (type === "text") {
    renderTextItemToBuffer(buffer, renderItem, container);
  } else if (type === "box") {
    renderBoxItemToBuffer(buffer, renderItem, container);
  } else {
    throw new Error(`unhandled render item type "${type}"`);
  }
}

function renderTextItemToBuffer(buffer: Character[][], renderItem: TextRenderItem, container: Geometry): void {
  const parser = createAnsiSequenceParser();
  const lines = renderItem.text.split("\n").map((line) => parser.parse(line));
  const containerBottom = container.top + container.height;
  const containerRight = container.left + container.width;

  let y = renderItem.geometry.top;
  for (let lineIndex = 0; lineIndex < renderItem.geometry.height; lineIndex++, y++) {
    if (y < container.top || y >= containerBottom) {
      continue;
    }

    const bufferRow = buffer[y];
    if (!bufferRow) {
      continue;
    }

    let x = renderItem.geometry.left;
    const tokens = lines[lineIndex];
    for (const token of tokens ?? []) {
      for (const ch of token.value) {
        if (x >= container.left && x < containerRight) {
          const bufferCh = bufferRow[x];
          if (bufferCh && renderItem.zIndex > bufferCh.zIndex) {
            bufferCh.value = ch ?? " ";
            bufferCh.color = token.foreground ? ansiColorToColorString(token.foreground) : renderItem.color;
            bufferCh.bgColor = token.background ? ansiColorToColorString(token.background) : renderItem.bgColor;
            bufferCh.inverse = renderItem.inverse ?? false;
            bufferCh.zIndex = renderItem.zIndex;
          }
        }
        x++;
      }
    }
  }
}

function renderBoxItemToBuffer(buffer: Character[][], renderItem: BoxRenderItem, container: Geometry): void {
  const top = renderItem.geometry.top;
  const left = renderItem.geometry.left;
  const bottom = top + renderItem.geometry.height;
  const right = left + renderItem.geometry.width;
  const containerBottom = container.top + container.height;
  const containerRight = container.left + container.width;
  let insideBorderContainer: Geometry | undefined;

  if (renderItem.borderLeftStyle && renderItem.borderLeftStyle !== "none" && renderItem.borderLeftColor) {
    const border = BORDERS[renderItem.borderLeftStyle];

    for (let y = top; y < bottom; y++) {
      if (y < container.top || y >= containerBottom) {
        continue;
      }

      const bufferRow = buffer[y];
      if (!bufferRow) {
        continue;
      }
      for (let x = left; x < right; x++) {
        if (x < container.left || x >= containerRight) {
          continue;
        }

        const bufferCh = bufferRow[x];
        if (!bufferCh) {
          continue;
        }

        bufferCh.bgColor = undefined;
        bufferCh.color = renderItem.borderLeftColor;
        bufferCh.inverse = false;
        if (x === left && y === top) {
          bufferCh.value = border.nw;
        } else if (x === left && y === bottom - 1) {
          bufferCh.value = border.sw;
        } else if (x === right - 1 && y === top) {
          bufferCh.value = border.ne;
        } else if (x === right - 1 && y === bottom - 1) {
          bufferCh.value = border.se;
        } else if (x === left) {
          bufferCh.value = border.w;
        } else if (x === right - 1) {
          bufferCh.value = border.e;
        } else if (y === top) {
          bufferCh.value = border.n;
        } else if (y === bottom - 1) {
          bufferCh.value = border.s;
        } else {
          bufferCh.value = " ";
        }
      }
    }

    insideBorderContainer = {
      left: container.left + 1,
      top: container.top + 1,
      width: container.width - 2,
      height: container.height - 2,
    };
  }

  for (const child of renderItem.children) {
    if (child.canRenderOnBorder) {
      renderItemToBuffer(buffer, child, container);
    } else {
      renderItemToBuffer(buffer, child, insideBorderContainer ?? container);
    }
  }
}

function createBuffer(width: number, height: number): Character[][] {
  const buffer: Character[][] = [];
  for (let y = 0; y < height; y++) {
    const line: Character[] = [];
    for (let x = 0; x < width; x++) {
      line.push({
        value: " ",
        color: "#ffffff",
        inverse: false,
        zIndex: Number.MIN_SAFE_INTEGER,
      });
    }
    buffer.push(line);
  }
  return buffer;
}
