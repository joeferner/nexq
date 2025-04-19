import { Key } from "readline";
import { Node as YogaNode } from "yoga-layout";
import { Component } from "../render/Component.js";
import { geometryFromYogaNode } from "../render/Geometry.js";
import { RenderItem } from "../render/RenderItem.js";
import { inputToChar, isInputMatch } from "../utils/input.js";

export interface InputBoxOptions {
  width: number;
  inputBoxFocusColor: string;
  inputBoxFocusBgColor: string;
}

export class InputBox extends Component {
  private _value = "";
  private offset = 0;
  private _cursorPosition = 0;

  public constructor(private readonly options: InputBoxOptions) {
    super();
    this.width = options.width;
    this.height = 1;
  }

  public override handleKeyPress(_chunk: string, key: Key | undefined): boolean {
    if (!this.focused || !key) {
      return false;
    }

    const ch = inputToChar(key);
    if (ch) {
      this.value = this.value.substring(0, this.cursorPosition) + ch + this.value.substring(this.cursorPosition);
      this.cursorPosition++;
      return true;
    }

    if (isInputMatch(key, "backspace")) {
      const atEnd = this.cursorPosition === this.value.length;
      this.value = this.value.substring(0, this.cursorPosition - 1) + this.value.substring(this.cursorPosition);
      if (!atEnd) {
        this.cursorPosition--;
      }
      return true;
    }

    if (isInputMatch(key, "delete")) {
      this.value = this.value.substring(0, this.cursorPosition) + this.value.substring(this.cursorPosition + 1);
      return true;
    }

    if (isInputMatch(key, "left")) {
      this.cursorPosition--;
      return true;
    }

    if (isInputMatch(key, "right")) {
      this.cursorPosition++;
      return true;
    }

    if (isInputMatch(key, "home")) {
      this.cursorPosition = 0;
      return true;
    }

    if (isInputMatch(key, "end")) {
      this.cursorPosition = this.value.length;
      return true;
    }

    return false;
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

  protected preRender(yogaNode: YogaNode): RenderItem[] {
    const results = super.preRender(yogaNode);
    const geometry = geometryFromYogaNode(yogaNode);
    const text = this.value.substring(this.offset, this.offset + geometry.width);

    results.push({
      type: "text",
      text: text + " ".repeat(geometry.width - text.length),
      color: this.options.inputBoxFocusColor,
      bgColor: this.focused ? this.options.inputBoxFocusBgColor : undefined,
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
