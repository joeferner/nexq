import { Node as YogaNode } from "yoga-layout";
import { Element } from "./Element.js";
import { geometryFromYogaNode } from "./Geometry.js";
import { RenderItem } from "./RenderItem.js";
import { Document } from "./Document.js";

export interface TextOptions {
  text: string;
  color?: string;
  inverse?: boolean;
}

export class Text extends Element {
  public text: string;
  public color: string | undefined;
  public inverse: boolean;

  public constructor(document: Document, options: TextOptions) {
    super(document);
    this.text = options.text;
    this.color = options.color;
    this.inverse = options.inverse ?? false;
  }

  public override populateLayout(container: YogaNode): void {
    // TODO wrap text
    const lines = this.text.split("\n");
    this.style.height = lines.length;
    this.style.width = Math.max(...lines.map((l) => l.length));
    super.populateLayout(container);
  }

  protected override preRender(yogaNode: YogaNode): RenderItem[] {
    return [
      {
        type: "text",
        text: this.text,
        color: this.color ?? "#ffffff",
        inverse: this.inverse,
        zIndex: this.zIndex,
        geometry: geometryFromYogaNode(yogaNode),
      },
    ];
  }
}
