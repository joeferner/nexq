import { Node as YogaNode } from "yoga-layout/load";
import { Component } from "./Component.js";
import { RenderItem } from "./RenderItem.js";
import Yoga from "yoga-layout";
import { geometryFromYogaNode } from "./Geometry.js";

export interface TextComponentOptions {
  text: string;
  color?: string;
  inverse?: boolean;
}

export class TextComponent extends Component {
  public text: string;
  public color: string | undefined;
  public inverse: boolean;

  public constructor(options: TextComponentOptions) {
    super();
    this.text = options.text;
    this.color = options.color;
    this.inverse = options.inverse ?? false;
  }

  public get children(): Component[] {
    return [];
  }

  public override populateLayout(container: YogaNode): void {
    // TODO wrap text
    const lines = this.text.split("\n");
    const height = lines.length;
    const width = Math.max(...lines.map((l) => l.length));
    this.yogaNode = Yoga.Node.create();
    this.yogaNode.setMinWidth(width);
    this.yogaNode.setWidth(width);
    this.yogaNode.setMaxWidth(width);
    this.yogaNode.setMinHeight(height);
    this.yogaNode.setHeight(height);
    this.yogaNode.setMaxHeight(height);
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public override render(): RenderItem[] {
    return [
      {
        type: "text",
        text: this.text,
        color: this.color ?? "#ffffff",
        inverse: this.inverse,
        zIndex: this.zIndex,
        geometry: geometryFromYogaNode(this.yogaNode),
      },
    ];
  }
}
