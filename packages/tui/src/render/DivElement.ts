import Yoga, { PositionType, Node as YogaNode } from "yoga-layout";
import { Element, RenderChildrenOptions } from "./Element.js";
import { BoxRenderItem } from "./RenderItem.js";
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

  protected override renderChildren(options: RenderChildrenOptions): void {
    if (options.parent.type !== "box") {
      throw new Error("expected render to create a box for us");
    }
    const box = options.parent as BoxRenderItem;

    box.borderLeftStyle = this.style.borderLeftStyle ?? "none";
    box.borderRightStyle = this.style.borderRightStyle ?? "none";
    box.borderTopStyle = this.style.borderTopStyle ?? "none";
    box.borderBottomStyle = this.style.borderBottomStyle ?? "none";
    box.borderLeftColor = this.style.borderLeftColor;
    box.borderRightColor = this.style.borderRightColor;
    box.borderTopColor = this.style.borderTopColor;
    box.borderBottomColor = this.style.borderBottomColor;
    box.zIndex = this.zIndex;

    let left = options.container.left;
    let top = options.container.top;
    let width = options.container.width;
    let height = options.container.height;
    if (box.borderLeftStyle !== "none") {
      left++;
      width--;
    }
    if (box.borderRightStyle !== "none") {
      width--;
    }
    if (box.borderTopStyle !== "none") {
      top++;
      height--;
    }
    if (box.borderBottomStyle !== "none") {
      height--;
    }

    super.renderChildren({
      ...options,
      container: {
        left,
        top,
        width,
        height,
      },
    });

    if (this.scrollTop || this.scrollLeft) {
      for (const child of box.children) {
        child.geometry.top -= this.scrollTop;
        child.geometry.left -= this.scrollLeft;
      }
    }

    if (this.borderTitleText) {
      const titleParent: BoxRenderItem = {
        type: "box",
        canRenderOnBorder: true,
        children: [],
        zIndex: this.zIndex + 1,
        name: this.id ? `${this.id}-title` : undefined,
        geometry: {
          top: 0,
          left: 0,
          width,
          height,
        },
      };
      options.parent.children.push(titleParent);
      this.borderTitleText.render({
        parent: titleParent,
        root: titleParent,
      });
      const titleWidth = this.borderTitleText.clientWidth;
      titleParent.geometry.left = Math.floor((this.clientWidth - titleWidth) / 2);
    }
  }
}
