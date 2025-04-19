import Yoga, { Align, FlexDirection, Justify } from "yoga-layout";
import { Node as YogaNode } from "yoga-layout/load";
import { RenderItem } from "./RenderItem.js";

export abstract class Component {
  protected yogaNode?: YogaNode;
  public height: number | 'auto' | `${number}%` | undefined;
  public width: number | 'auto' | `${number}%` | undefined;
  public flexGrow: number | undefined;
  public alignItems = Align.Auto;
  public direction = FlexDirection.Row;
  public justifyContent = Justify.FlexStart;

  public render(): RenderItem[] {
    if (!this.yogaNode) {
      return [];
    }

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

  public populateLayout(container: YogaNode): void {
    this.yogaNode = Yoga.Node.create();
    this.yogaNode.setHeight(this.height);
    this.yogaNode.setWidth(this.width);
    this.yogaNode.setFlexGrow(this.flexGrow);
    this.yogaNode.setAlignItems(this.alignItems);
    this.yogaNode.setFlexDirection(this.direction);
    this.yogaNode.setJustifyContent(this.justifyContent);
    for (const child of this.children) {
      child.populateLayout(this.yogaNode);
    }
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public get zIndex(): number {
    return 0;
  }

  public abstract get children(): Component[];
}
