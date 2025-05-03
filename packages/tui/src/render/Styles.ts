import { Align, Display, Edge, FlexDirection, Justify, Overflow, PositionType, Node as YogaNode } from "yoga-layout";

export class Styles {
  public height: number | "auto" | `${number}%` | undefined;
  public minHeight: number | `${number}%` | undefined;
  public maxHeight: number | `${number}%` | undefined;
  public width: number | "auto" | `${number}%` | undefined;
  public minWidth: number | `${number}%` | undefined;
  public maxWidth: number | `${number}%` | undefined;
  public flexGrow: number | undefined;
  public flexShrink: number | undefined;
  public alignItems = Align.FlexStart;
  public flexDirection = FlexDirection.Row;
  public justifyContent = Justify.FlexStart;
  public positionType = PositionType.Relative;
  public display = Display.Flex;
  public marginLeft?: number | "auto" | `${number}%` | undefined;
  public marginRight?: number | "auto" | `${number}%` | undefined;
  public marginTop?: number | "auto" | `${number}%` | undefined;
  public marginBottom?: number | "auto" | `${number}%` | undefined;
  public paddingLeft?: number | `${number}%` | undefined;
  public paddingRight?: number | `${number}%` | undefined;
  public paddingTop?: number | `${number}%` | undefined;
  public paddingBottom?: number | `${number}%` | undefined;
  public overflow?: Overflow;
  public color?: string;
  public inverse?: boolean;

  public set margin(margin: number | "auto" | `${number}%` | undefined) {
    this.marginLeft = margin;
    this.marginRight = margin;
    this.marginTop = margin;
    this.marginBottom = margin;
  }

  public apply(node: YogaNode): void {
    node.setOverflow(this.overflow ?? Overflow.Visible);

    node.setHeight(this.height);
    node.setMinHeight(this.minHeight);
    node.setMaxHeight(this.maxHeight);

    node.setWidth(this.width);
    node.setMinWidth(this.minWidth);
    node.setMaxWidth(this.maxWidth);

    node.setFlexGrow(this.flexGrow);
    node.setFlexShrink(this.flexShrink);
    node.setAlignItems(this.alignItems);
    node.setFlexDirection(this.flexDirection);
    node.setJustifyContent(this.justifyContent);
    node.setPositionType(this.positionType);
    node.setMargin(Edge.Left, this.marginLeft);
    node.setMargin(Edge.Right, this.marginRight);
    node.setMargin(Edge.Top, this.marginTop);
    node.setMargin(Edge.Bottom, this.marginBottom);
    node.setPadding(Edge.Left, this.paddingLeft);
    node.setPadding(Edge.Right, this.paddingRight);
    node.setPadding(Edge.Top, this.paddingTop);
    node.setPadding(Edge.Bottom, this.paddingBottom);
    node.setDisplay(this.display);
  }
}
