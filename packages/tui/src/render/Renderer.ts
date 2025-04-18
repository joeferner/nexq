import { cursorTo } from "ansi-escapes";
import * as ansis from "ansis";
import * as R from "radash";
import { Component } from "./Component.js";
import { RenderItem, TextRenderItem } from "./RenderItem.js";
import { createLogger } from "../utils/logger.js";

const logger = createLogger("Renderer");

interface Character {
  value: string;
  color: string;
  inverse: boolean;
}

function isCharacterEqual(ch1: Character, ch2: Character | undefined): boolean {
  if (ch2 === undefined) {
    return false;
  }
  return ch1.value === ch2.value && ch1.color === ch2.color && ch1.inverse === ch2.inverse;
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

    component.calculateGeometry({ top: 0, left: 0, width: this.width, height: this.height });
    const renderItems = component.render();
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
    return `${ch.color}${ch.inverse ? "t" : "f"}`;
  };

  const getAnsi = (key: string, ch: Character): ansis.Ansis => {
    const existing = ansiCache[key];
    if (existing) {
      return existing;
    }
    let ansi = ansis.hex(ch.color);
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
  if (type === "text") {
    renderTextItemToBuffer(buffer, renderItem);
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
      bufferCh.inverse = renderItem.inverse ?? false;
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
