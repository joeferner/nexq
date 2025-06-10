import {
  Align,
  Display,
  Edge,
  FlexDirection,
  Gutter,
  Justify,
  Overflow,
  PositionType,
  Wrap,
  Node as YogaNode,
} from "yoga-layout";

export type BorderStyle = "none" | "solid";

export class Style {
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
  public flexWrap = Wrap.NoWrap;
  public columnGap: number | `${number}%` | undefined;
  public rowGap: number | `${number}%` | undefined;
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

  public borderLeftColor = "#ffffff";
  public borderRightColor = "#ffffff";
  public borderTopColor = "#ffffff";
  public borderBottomColor = "#ffffff";

  public borderLeftStyle?: BorderStyle;
  public borderRightStyle?: BorderStyle;
  public borderTopStyle?: BorderStyle;
  public borderBottomStyle?: BorderStyle;

  public overflowX?: Overflow;
  public overflowY?: Overflow;

  public set overflow(overflow: Overflow | undefined) {
    this.overflowX = overflow;
    this.overflowY = overflow;
  }

  public set margin(margin: number | "auto" | `${number}%` | undefined) {
    this.marginLeft = margin;
    this.marginRight = margin;
    this.marginTop = margin;
    this.marginBottom = margin;
  }

  public set borderColor(borderColor: string) {
    this.borderLeftColor = borderColor;
    this.borderRightColor = borderColor;
    this.borderTopColor = borderColor;
    this.borderBottomColor = borderColor;
  }

  public set borderStyle(borderStyle: BorderStyle) {
    this.borderLeftStyle = borderStyle;
    this.borderRightStyle = borderStyle;
    this.borderTopStyle = borderStyle;
    this.borderBottomStyle = borderStyle;
  }

  public apply(node: YogaNode): void {
    node.setOverflow(this.overflowY ?? this.overflowX ?? Overflow.Visible);

    node.setHeight(this.height);
    node.setMinHeight(this.minHeight);
    node.setMaxHeight(this.maxHeight);

    node.setWidth(this.width);
    node.setMinWidth(this.minWidth);
    node.setMaxWidth(this.maxWidth);

    node.setBorder(Edge.Left, borderWidth(this.borderLeftStyle));
    node.setBorder(Edge.Right, borderWidth(this.borderRightStyle));
    node.setBorder(Edge.Top, borderWidth(this.borderTopStyle));
    node.setBorder(Edge.Bottom, borderWidth(this.borderBottomStyle));

    node.setFlexGrow(this.flexGrow);
    node.setFlexShrink(this.flexShrink);
    node.setAlignItems(this.alignItems);
    node.setFlexDirection(this.flexDirection);
    node.setFlexWrap(this.flexWrap);
    node.setJustifyContent(this.justifyContent);
    node.setPositionType(this.positionType);
    node.setGap(Gutter.Column, this.columnGap);
    node.setGap(Gutter.Row, this.rowGap);

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

function borderWidth(style: BorderStyle | undefined): number {
  return style === "none" || !style ? 0 : 1;
}
