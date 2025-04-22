import Yoga, { PositionType, Node as YogaNode } from "yoga-layout";
import { Element } from "./Element.js";
import { BorderType, RenderItem } from "./RenderItem.js";
import { Document } from "./Document.js";

export class Box extends Element {
  public borderType = BorderType.Single;
  public borderColor = "#ffffff";
  public title: Element | undefined;

  public constructor(document: Document) {
    super(document);
    this.style.margin = 1;
  }

  public override populateLayout(container: YogaNode): void {
    super.populateLayout(container);

    if (this.title) {
      const titleContainer = Yoga.Node.create();
      titleContainer.setPositionType(PositionType.Absolute);
      this.title.populateLayout(titleContainer);
      container.insertChild(titleContainer, container.getChildCount());
    }
  }

  protected override preRender(yogaNode: YogaNode): RenderItem[] {
    const renderItems: RenderItem[] = [];

    renderItems.push({
      type: "box",
      borderType: this.borderType,
      color: this.borderColor,
      zIndex: this.zIndex - 0.001,
      geometry: {
        left: yogaNode.getComputedLeft() - 1,
        top: yogaNode.getComputedTop() - 1,
        width: yogaNode.getComputedWidth() + 2,
        height: yogaNode.getComputedHeight() + 2,
      },
    });

    if (this.title) {
      const titleRenderItems = this.title.render();
      const titleWidth = this.title.computedWidth;
      const offset = Math.floor((this.computedWidth - titleWidth) / 2);
      for (const titleRenderItem of titleRenderItems) {
        titleRenderItem.geometry.left += offset;
        renderItems.push(titleRenderItem);
      }
    }

    return renderItems;
  }
}
