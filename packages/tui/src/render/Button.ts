import { isInputMatch } from "../utils/input.js";
import { Document } from "./Document.js";
import { Element, ElementEvents } from "./Element.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { Text } from "./Text.js";

export interface ButtonOptions {
  text: string;
  color?: string;
  selectedColor?: string;
}

export interface ButtonEvents extends ElementEvents {
  click: () => unknown;
}

export class Button extends Element {
  private readonly textElement: Text;
  public color: string;
  public selectedColor: string;

  public constructor(document: Document, options: ButtonOptions) {
    super(document);
    this.color = options.color ?? "#ffffff";
    this.selectedColor = options.selectedColor ?? "#ffffff";
    this.textElement = new Text(document, { text: options.text, color: this.color });
    this.appendChild(this.textElement);
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "return")) {
      this.eventEmitter.emit("click");
      return;
    }

    super.onKeyDown(event);
  }

  protected override onFocus(): void {
    this.textElement.style.color = this.selectedColor;
    this.textElement.style.inverse = true;
    super.onFocus();
  }

  protected override onBlur(): void {
    this.textElement.style.color = this.color;
    this.textElement.style.inverse = false;
    super.onBlur();
  }

  public addEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.addListener(event, listener);
  }

  public removeEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.removeListener(event, listener);
  }
}
