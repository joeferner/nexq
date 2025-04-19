import { Node } from "yoga-layout";
import { Component } from "../render/Component.js";
import { Text } from "./Text.js";

export interface ButtonOptions {
  text: string;
}

export class Button extends Component {
  private readonly textComponent: Text;

  public constructor(options: ButtonOptions) {
    super();
    this.textComponent = new Text({ text: options.text });
    this.children.push(this.textComponent);
  }

  public override populateLayout(container: Node): void {
    this.textComponent.inverse = this.focused;
    super.populateLayout(container);
  }
}
