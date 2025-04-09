import chalk, { ChalkInstance } from "chalk";
import { Geometry } from "./Geometry.js";
import { RenderItem, TextRenderItem } from "./RenderItem.js";
import * as R from "radash";
import { cursorTo } from "ansi-escapes";

interface Character {
    value: string;
    color: string;
}

export abstract class Component {
    public getGeometry(): Geometry | undefined {
        return undefined;
    }
    public getRenderItems(): RenderItem[] {
        return [];
    }

    public abstract getChildren(): Component[];
}

export class Renderer {
    private _width = 0;
    private _height = 0;
    private previousWidth = 0;
    private previousHeight = 0;
    private lastBuffer?: Character[][];
    private buffer?: Character[][];

    public get width(): number {
        return this._width;
    }

    public get height(): number {
        return this._height;
    }

    public render(component: Component): void {
        const renderItems: RenderItem[] = [];
        walkComponent(component, renderItems);
        const sortedRenderItems = R.sort(renderItems, r => r.zIndex)

        this._width = process.stdout.columns;
        this._height = process.stdout.rows;
        if (!this.buffer || this._width !== this.previousWidth || this._height !== this.previousHeight) {
            this.buffer = createBuffer(this._width, this._height);
        }

        for (const renderItem of sortedRenderItems) {
            renderItemToBuffer(this.buffer, renderItem);
        }

        renderBuffer(this.buffer, this.lastBuffer);

        this.previousWidth = this._width;
        this.previousHeight = this._height;
        this.lastBuffer = this.buffer;
    }
}

function renderBuffer(buffer: Character[][], lastBuffer?: Character[][]): void {
    const ansiCache: Record<string, ChalkInstance> = {};

    const getAnsi = (ch: Character): ChalkInstance => {
        const key = ch.color;
        const existing = ansiCache[key];
        if (existing) {
            return existing;
        }
        const ansi = chalk.hex(ch.color);
        ansiCache[key] = ansi;
        return ansi;
    };

    for (let y = 0; y < buffer.length; y++) {
        const row = buffer[y];
        process.stdout.write(cursorTo(0, y + 1));
        for (let x = 0; x < row.length; x++) {
            const ch = row[x];
            const ansi = getAnsi(ch);
            process.stdout.write(ansi(ch.value));
        }
    }
}

function renderItemToBuffer(buffer: Character[][], renderItem: RenderItem): void {
    const type = renderItem.type;
    if (type === 'text') {
        renderTextItemToBuffer(buffer, renderItem);
    } else {
        throw new Error(`unhandled render item type "${type}"`);
    }
}

function renderTextItemToBuffer(buffer: Character[][], renderItem: TextRenderItem): void {
    const lines = renderItem.text.split('\n');
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
            bufferCh.value = ch ?? ' ';
            bufferCh.color = renderItem.color;
        }
    }
}

function walkComponent(component: Component, renderItems: RenderItem[]): void {
    const componentRenderItems = component.getRenderItems();
    renderItems.push(...componentRenderItems);
    for (const child of component.getChildren()) {
        walkComponent(child, renderItems);
    }
}

function createBuffer(width: number, height: number): Character[][] {
    const buffer: Character[][] = [];
    for (let y = 0; y < height; y++) {
        const line: Character[] = [];
        for (let x = 0; x < width; x++) {
            line.push({
                value: ' ',
                color: '#ffffff'
            });
        }
        buffer.push(line);
    }
    return buffer;
}