import { Node } from "yoga-layout";
import { isInputMatch } from "../utils/input.js";
import { Document } from "./Document.js";
import { Element, ElementEvents } from "./Element.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { Text } from "./Text.js";
import { Style } from "./Style.js";

export interface ButtonEvents extends ElementEvents {
  click: () => unknown;
}

export class ButtonStyle extends Style {
  public color?: string;
}

export class Button extends Element {
  private readonly textElement: Text;
  public selectedColor: string;

  public constructor(document: Document) {
    super(document);
    this.style.color = "#ffffff";
    this.selectedColor = "#ffffff";
    this.textElement = new Text(document, { text: "" });
    this.appendChild(this.textElement);
  }

  protected override createStyle(): Style {
    return new ButtonStyle();
  }

  public override get style(): ButtonStyle {
    return super.style;
  }

  public get text(): string {
    return this.textElement.text;
  }

  public set text(text: string) {
    this.textElement.text = text;
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "return")) {
      this.eventEmitter.emit("click");
      return;
    }

    super.onKeyDown(event);
  }

  public override populateLayout(container: Node): void {
    super.populateLayout(container);
    if (this.window.activeElement === this) {
      this.textElement.style.color = this.selectedColor;
      this.textElement.style.inverse = true;
    } else {
      this.textElement.style.color = this.style.color;
      this.textElement.style.inverse = false;
    }
  }

  public addEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.addListener(event, listener);
  }

  public removeEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.removeListener(event, listener);
  }
}
