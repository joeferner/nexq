import Yoga, { PositionType, Node as YogaNode } from "yoga-layout";
import { Document } from "./Document.js";
import { Element, PreRenderOptions } from "./Element.js";
import { BorderType, RenderItem } from "./RenderItem.js";
import { Text } from "./Text.js";

export class Box extends Element {
  public borderType = BorderType.Single;
  public borderColor = "#ffffff";
  private titleText: Text | undefined;

  public get title(): string | undefined {
    return this.titleText?.text;
  }

  public set title(title: string | undefined) {
    if (title === undefined) {
      this.titleText = undefined;
    } else {
      if (!this.titleText) {
        this.titleText = new Text(this.document, { text: title });
      } else {
        this.titleText.text = title;
      }
    }
  }

  public constructor(document: Document) {
    super(document);
    this.style.margin = 1;
  }

  public override populateLayout(container: YogaNode): void {
    super.populateLayout(container);

    if (this.titleText) {
      const titleContainer = Yoga.Node.create();
      titleContainer.setPositionType(PositionType.Absolute);
      this.titleText.populateLayout(titleContainer);
      container.insertChild(titleContainer, container.getChildCount());
    }
  }

  protected override preRender(options: PreRenderOptions): RenderItem[] {
    const { container, geometry } = options;
    const renderItems: RenderItem[] = [];

    renderItems.push({
      type: "box",
      borderType: this.borderType,
      color: this.borderColor,
      zIndex: this.zIndex - 0.001,
      container,
      geometry: {
        top: geometry.top - 1,
        left: geometry.left - 1,
        height: geometry.height + 2,
        width: geometry.width + 2,
      },
    });

    if (this.titleText) {
      const titleRenderItems = this.titleText.render({ ...container, top: container.top });
      const titleWidth = this.titleText.computedWidth;
      const offset = Math.floor((this.computedWidth - titleWidth) / 2);
      for (const titleRenderItem of titleRenderItems) {
        titleRenderItem.geometry.left += offset;
        renderItems.push(titleRenderItem);
      }
    }

    return renderItems;
  }
}
