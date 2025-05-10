import Yoga, { PositionType, Node as YogaNode } from "yoga-layout";
import { Element, RenderOptions } from "./Element.js";
import { RenderItem } from "./RenderItem.js";
import { Text } from "./Text.js";

export class DivElement extends Element {
  private borderTitleText?: Text;

  public get borderTitle(): string | undefined {
    return this.borderTitleText?.text;
  }

  public set borderTitle(borderTitle: string | undefined) {
    if (borderTitle === undefined) {
      this.borderTitleText = undefined;
    } else {
      if (!this.borderTitleText) {
        this.borderTitleText = new Text(this.document, { text: borderTitle });
      } else {
        this.borderTitleText.text = borderTitle;
      }
    }
  }

  public override populateLayout(container: YogaNode): void {
    super.populateLayout(container);

    if (this.borderTitleText) {
      const titleContainer = Yoga.Node.create();
      titleContainer.setPositionType(PositionType.Absolute);
      this.borderTitleText.populateLayout(titleContainer);
      container.insertChild(titleContainer, container.getChildCount());
    }
  }

  protected override _render(options: RenderOptions): RenderItem[] {
    const { outerContainer, outerGeometry } = options;

    const renderItems = super._render(options);

    renderItems.push({
      type: "box",
      borderLeftStyle: this.style.borderLeftStyle ?? "none",
      borderRightStyle: this.style.borderRightStyle ?? "none",
      borderTopStyle: this.style.borderTopStyle ?? "none",
      borderBottomStyle: this.style.borderBottomStyle ?? "none",
      borderLeftColor: this.style.borderLeftColor,
      borderRightColor: this.style.borderRightColor,
      borderTopColor: this.style.borderTopColor,
      borderBottomColor: this.style.borderBottomColor,
      zIndex: this.zIndex - 0.001,
      container: outerContainer,
      geometry: outerGeometry,
    });

    if (this.borderTitleText) {
      const titleRenderItems = this.borderTitleText.render({
        ...options,
        innerContainer: outerContainer,
        innerGeometry: outerGeometry,
      });
      const titleWidth = this.borderTitleText.clientWidth;
      const offset = Math.floor((this.clientWidth - titleWidth) / 2);
      for (const titleRenderItem of titleRenderItems) {
        titleRenderItem.geometry.left += offset;
        titleRenderItem.zIndex = this.zIndex;
        renderItems.push(titleRenderItem);
      }
    }

    return renderItems;
  }
}
