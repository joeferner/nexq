import { Node } from "yoga-layout";
import { isInputMatch } from "../utils/input.js";
import { Document } from "./Document.js";
import { Element, ElementEvents } from "./Element.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { Text } from "./Text.js";

export interface ButtonOptions {
  text: string;
}

export interface ButtonEvents extends ElementEvents {
  click: () => unknown;
}

export class Button extends Element {
  private readonly textElement: Text;

  public constructor(document: Document, options: ButtonOptions) {
    super(document);
    this.textElement = new Text(document, { text: options.text });
    this.appendChild(this.textElement);
  }

  public override populateLayout(container: Node): void {
    this.textElement.inverse = this.focused;
    super.populateLayout(container);
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "return")) {
      this.eventEmitter.emit("click");
      return;
    }

    super.onKeyDown(event);
  }

  public addEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.addListener(event, listener);
  }

  public removeEventListener<T extends keyof ButtonEvents>(event: T, listener: ButtonEvents[T]): void {
    this.eventEmitter.removeListener(event, listener);
  }
}
