import Yoga, { PositionType, Node as YogaNode } from "yoga-layout";
import { Component } from "../render/Component.js";
import { BorderType, RenderItem } from "../render/RenderItem.js";

export class Box extends Component {
  public borderType = BorderType.Single;
  public borderColor = "#ffffff";
  public title: Component | undefined;

  public constructor() {
    super();
    this.margin = { left: 1, bottom: 1, right: 1, top: 1 };
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
