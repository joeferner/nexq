import { Node as YogaNode } from "yoga-layout";
import { Component } from "../render/Component.js";
import { geometryFromYogaNode } from "../render/Geometry.js";
import { RenderItem } from "../render/RenderItem.js";

export interface TextOptions {
  text: string;
  color?: string;
  inverse?: boolean;
}

export class Text extends Component {
  public text: string;
  public color: string | undefined;
  public inverse: boolean;

  public constructor(options: TextOptions) {
    super();
    this.text = options.text;
    this.color = options.color;
    this.inverse = options.inverse ?? false;
  }

  public override populateLayout(container: YogaNode): void {
    // TODO wrap text
    const lines = this.text.split("\n");
    this.height = lines.length;
    this.width = Math.max(...lines.map((l) => l.length));
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
