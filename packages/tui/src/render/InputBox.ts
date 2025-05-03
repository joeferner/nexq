import { inputToChar, isInputMatch } from "../utils/input.js";
import { Document } from "./Document.js";
import { Element, PreRenderOptions } from "./Element.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { RenderItem } from "./RenderItem.js";

export interface InputBoxOptions {
  color?: string;
  bgColor?: string;
  focusColor?: string;
  focusBgColor?: string;
}

export class InputBox extends Element {
  private _value = "";
  private offset = 0;
  private _cursorPosition = 0;

  public constructor(
    document: Document,
    private readonly options: InputBoxOptions
  ) {
    super(document);
    this.style.height = 1;
  }

  public override onKeyDown(event: KeyboardEvent): void {
    const ch = inputToChar(event);
    if (ch) {
      this.value = this.value.substring(0, this.cursorPosition) + ch + this.value.substring(this.cursorPosition);
      this.cursorPosition++;
      return;
    }

    if (isInputMatch(event, "backspace")) {
      const atEnd = this.cursorPosition === this.value.length;
      this.value = this.value.substring(0, this.cursorPosition - 1) + this.value.substring(this.cursorPosition);
      if (!atEnd) {
        this.cursorPosition--;
      }
      return;
    }

    if (isInputMatch(event, "delete")) {
      this.value = this.value.substring(0, this.cursorPosition) + this.value.substring(this.cursorPosition + 1);
      return;
    }

    if (isInputMatch(event, "left")) {
      this.cursorPosition--;
      return;
    }

    if (isInputMatch(event, "right")) {
      this.cursorPosition++;
      return;
    }

    if (isInputMatch(event, "home")) {
      this.cursorPosition = 0;
      return;
    }

    if (isInputMatch(event, "end")) {
      this.cursorPosition = this.value.length;
      return;
    }

    super.onKeyDown(event);
  }

  public get value(): string {
    return this._value;
  }

  public set value(value: string) {
    const cursorPosition = this.cursorPosition;
    this._value = value;
    this.cursorPosition = cursorPosition; // trigger setter
  }

  public get cursorPosition(): number {
    return this._cursorPosition;
  }

  public set cursorPosition(cursorPosition: number) {
    const width = Math.max(1, this.computedWidth);
    this._cursorPosition = Math.max(0, Math.min(cursorPosition, this.value.length));
    if (this._cursorPosition - this.offset >= width) {
      this.offset = this._cursorPosition - width + 1;
    } else if (this._cursorPosition < this.offset) {
      this.offset = this._cursorPosition;
    }
  }

  protected preRender(options: PreRenderOptions): RenderItem[] {
    const { container, geometry } = options;
    const results = super.preRender(options);
    const text = this.value.substring(this.offset, this.offset + geometry.width);

    results.push({
      type: "text",
      text: text + " ".repeat(geometry.width - text.length),
      color: this.focused ? (this.options.focusColor ?? "#ffffff") : (this.options.color ?? "#ffffff"),
      bgColor: this.focused ? this.options.focusBgColor : this.options.bgColor,
      container,
      geometry,
      zIndex: this.zIndex,
    });
    if (this.focused) {
      results.push({
        type: "cursor",
        geometry: { left: geometry.left + this.cursorPosition - this.offset, top: geometry.top, width: 1, height: 1 },
        zIndex: this.zIndex,
      });
    }

    return results;
  }
}
