import Yoga, { Align, Edge, FlexDirection, Justify } from "yoga-layout";
import { Node as YogaNode } from "yoga-layout/load";
import { RenderItem } from "./RenderItem.js";

export class Component {
  protected yogaNode?: YogaNode;
  public height: number | 'auto' | `${number}%` | undefined;
  public width: number | 'auto' | `${number}%` | undefined;
  public flexGrow: number | undefined;
  public alignItems = Align.FlexStart;
  public flexDirection = FlexDirection.Row;
  public justifyContent = Justify.FlexStart;
  public margin: number | 'auto' | `${number}%` | undefined;
  public children: Component[] = [];
  public zIndex = 0;
  protected _computedWidth = 0;
  protected _computedHeight = 0;

  public populateLayout(container: YogaNode): void {
    this.yogaNode = Yoga.Node.create();
    this.yogaNode.setHeight(this.height);
    this.yogaNode.setWidth(this.width);
    this.yogaNode.setFlexGrow(this.flexGrow);
    this.yogaNode.setAlignItems(this.alignItems);
    this.yogaNode.setFlexDirection(this.flexDirection);
    this.yogaNode.setJustifyContent(this.justifyContent);
    this.yogaNode.setMargin(Edge.All, this.margin);
    for (const child of this.children) {
      child.populateLayout(this.yogaNode);
    }
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public render(): RenderItem[] {
    if (!this.yogaNode) {
      return [];
    }

    this._computedWidth = this.yogaNode.getComputedWidth();
    this._computedHeight = this.yogaNode.getComputedHeight();

    const renderItems: RenderItem[] = [];
    const x = this.yogaNode.getComputedLeft();
    const y = this.yogaNode.getComputedTop();
    for (const child of this.children) {
      const childRenderItems = child.render();
      for (const childRenderItem of childRenderItems) {
        childRenderItem.zIndex += this.zIndex;
        childRenderItem.geometry.left += x;
        childRenderItem.geometry.top += y;
        renderItems.push(childRenderItem);
      }
    }
    return renderItems;
  }

  public get computedWidth(): number {
    return this._computedWidth;
  }

  public get computedHeight(): number {
    return this._computedHeight;
  }
}
