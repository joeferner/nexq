import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { RenderItem } from "../render/RenderItem.js";
import { inputToChar, isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";

const logger = createLogger("InputBox");

export interface InputBoxOptions {
  width: number;
}

export class InputBox extends Component {
  public focus = false;
  public width: number;
  private _value = "";
  private offset = 0;
  private _cursorPosition = 0;

  public constructor(
    private readonly state: NexqState,
    options: InputBoxOptions
  ) {
    super();
    this.width = options.width;
    this.state.on("keypress", (_chunk, key) => {
      if (!this.focus || !key) {
        return;
      }

      const ch = inputToChar(key);
      if (ch) {
        this.value = this.value.substring(0, this.cursorPosition) + ch + this.value.substring(this.cursorPosition);
        this.cursorPosition++;
      } else if (isInputMatch(key, "backspace")) {
        const atEnd = this.cursorPosition === this.value.length;
        this.value = this.value.substring(0, this.cursorPosition - 1) + this.value.substring(this.cursorPosition);
        if (!atEnd) {
          this.cursorPosition--;
        }
      } else if (isInputMatch(key, "delete")) {
        this.value = this.value.substring(0, this.cursorPosition) + this.value.substring(this.cursorPosition + 1);
      } else if (isInputMatch(key, "left")) {
        this.cursorPosition--;
      } else if (isInputMatch(key, "right")) {
        this.cursorPosition++;
      } else if (isInputMatch(key, "home")) {
        this.cursorPosition = 0;
      } else if (isInputMatch(key, "end")) {
        this.cursorPosition = this.value.length;
      } else {
        logger.info("unhandled input", JSON.stringify(key));
      }
      this.state.emit("changed");
    });
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
    this._cursorPosition = Math.max(0, Math.min(cursorPosition, this.value.length));
    if (this._cursorPosition - this.offset >= this.width) {
      this.offset = this._cursorPosition - this.width + 1;
    } else if (this._cursorPosition < this.offset) {
      this.offset = this._cursorPosition;
    }
  }

  public get children(): Component[] {
    return [];
  }

  public render(): RenderItem[] {
    const text = this.value.substring(this.offset, this.offset + this.width);

    const items: RenderItem[] = [
      {
        type: "text",
        text: text + " ".repeat(this.width - text.length),
        color: this.state.inputBoxFocusColor,
        bgColor: this.focus ? this.state.inputBoxFocusBgColor : undefined,
        geometry: this.geometry,
        zIndex: this.zIndex,
      },
    ];
    if (this.focus) {
      items.push({
        type: "cursor",
        geometry: { left: this.cursorPosition - this.offset, top: 0, width: 1, height: 1 },
        zIndex: this.zIndex,
      });
    }
    return items;
  }

  public get geometry(): Geometry {
    return { left: 0, top: 0, height: 1, width: this.width };
  }
}
