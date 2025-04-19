import { cursorHide, cursorShow, cursorTo } from "ansi-escapes";
import * as ansis from "ansis";
import * as R from "radash";
import Yoga, { Direction } from "yoga-layout";
import { createLogger } from "../utils/logger.js";
import { Component } from "./Component.js";
import { BorderType, BoxRenderItem, RenderItem, TextRenderItem } from "./RenderItem.js";

const logger = createLogger("Renderer");

interface Character {
  value: string;
  color: string;
  bgColor?: string;
  inverse: boolean;
}

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

  public render(component: Component): void {
    const startTime = Date.now();
    this._width = process.stdout.columns ?? 80;
    this._height = process.stdout.rows ?? 40;

    const root = Yoga.Node.create();
    let renderItems;
    try {
      root.setWidth(this.width);
      root.setHeight(this.height);
      component.populateLayout(root);
      root.calculateLayout(undefined, undefined, Direction.LTR);
      renderItems = component.render();
    } finally {
      root.freeRecursive();
    }
    const sortedRenderItems = R.sort(renderItems, (r) => r.zIndex);

    if (!this.buffers[this.bufferIndex] || this._width !== this.previousWidth || this._height !== this.previousHeight) {
      this.buffers[this.bufferIndex] = createBuffer(this._width, this._height);
    }

    clearBuffer(this.buffers[this.bufferIndex]);
    for (const renderItem of sortedRenderItems) {
      renderItemToBuffer(this.buffers[this.bufferIndex], renderItem);
    }

    const secondaryBufferIndex = (this.bufferIndex + 1) % 2;
    renderBuffer(this.buffers[this.bufferIndex], this.buffers[secondaryBufferIndex]);

    const cursorRenderItem = sortedRenderItems.find((r) => r.type === "cursor");
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

function clearBuffer(buffer: Character[][]): void {
  for (let y = 0; y < buffer.length; y++) {
    const row = buffer[y];
    for (let x = 0; x < row.length; x++) {
      row[x].value = " ";
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
    let ansi = ansis.hex(ch.color);
    if (ch.bgColor) {
      ansi = ansi.bgHex(ch.bgColor);
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

function renderItemToBuffer(buffer: Character[][], renderItem: RenderItem): void {
  const type = renderItem.type;
  if (type === "cursor") {
    // nothing todo
  } else if (type === "text") {
    renderTextItemToBuffer(buffer, renderItem);
  } else if (type === "box") {
    renderBoxItemToBuffer(buffer, renderItem);
  } else {
    throw new Error(`unhandled render item type "${type}"`);
  }
}

function renderTextItemToBuffer(buffer: Character[][], renderItem: TextRenderItem): void {
  const lines = renderItem.text.split("\n");
  let y = renderItem.geometry.top;
  for (let lineIndex = 0; lineIndex < renderItem.geometry.height; lineIndex++, y++) {
    const bufferRow = buffer[y];
    if (!bufferRow) {
      continue;
    }

    let x = renderItem.geometry.left;
    const line = lines[lineIndex];
    for (let chIndex = 0; chIndex < renderItem.geometry.width; chIndex++, x++) {
      const bufferCh = bufferRow[x];
      if (!bufferCh) {
        continue;
      }

      const ch = line?.[chIndex];
      bufferCh.value = ch ?? " ";
      bufferCh.color = renderItem.color;
      bufferCh.bgColor = renderItem.bgColor;
      bufferCh.inverse = renderItem.inverse ?? false;
    }
  }
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

const BORDERS: Record<BorderType, BorderCharacters> = {
  [BorderType.Single]: {
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

function renderBoxItemToBuffer(buffer: Character[][], renderItem: BoxRenderItem): void {
  const { top, left, width, height } = renderItem.geometry;
  const border = BORDERS[renderItem.borderType]

  for (let y = 0; y < height; y++) {
    const bufferRow = buffer[top + y];
    if (!bufferRow) {
      continue;
    }
    for (let x = 0; x < width; x++) {
      const bufferCh = bufferRow[left + x];
      if (!bufferCh) {
        continue;
      }

      bufferCh.bgColor = undefined;
      bufferCh.color = renderItem.color;
      bufferCh.inverse = false;
      if (x === 0 && y === 0) {
        bufferCh.value = border.nw;
      } else if (x === 0 && y === height - 1) {
        bufferCh.value = border.sw;
      } else if (x === width - 1 && y === 0) {
        bufferCh.value = border.ne;
      } else if (x === width - 1 && y === height - 1) {
        bufferCh.value = border.se;
      } else if (x === 0) {
        bufferCh.value = border.w;
      } else if (x === width - 1) {
        bufferCh.value = border.e;
      } else if (y === 0) {
        bufferCh.value = border.n;
      } else if (y === height - 1) {
        bufferCh.value = border.s;
      } else {
        bufferCh.value = ' ';
      }
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
      });
    }
    buffer.push(line);
  }
  return buffer;
}
