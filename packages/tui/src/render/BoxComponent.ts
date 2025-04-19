import Yoga, { Align, Edge, Justify, Node, PositionType } from "yoga-layout";
import { FlexDirection } from "yoga-layout/load";
import { Component } from "./Component.js";
import { BorderType, RenderItem } from "./RenderItem.js";

export interface BoxComponentOptions {
  direction?: FlexDirection;
  alignItems?: Align;
  justifyContent?: Justify;
  flexGrow?: number;
  height?: number | 'auto' | `${number}%` | undefined;
  width?: number | 'auto' | `${number}%` | undefined;
  children: Component[];
  border?: BorderType;
  borderColor?: string;
  background?: boolean;
  zIndex?: number;
  title?: Component;
}

export class BoxComponent extends Component {
  public children: Component[];
  public borderType: BorderType;
  public borderColor: string;
  public background: boolean;
  public title: Component | undefined;

  public constructor(options?: BoxComponentOptions) {
    super();
    options = options ?? {
      children: [],
    } satisfies BoxComponentOptions;
    this.children = options.children;
    this.flexDirection = options.direction ?? FlexDirection.Row;
    this.alignItems = options.alignItems ?? Align.Auto;
    this.flexGrow = options.flexGrow;
    this.justifyContent = options.justifyContent ?? Justify.FlexStart;
    this.height = options.height;
    this.width = options.width;
    this.borderType = options.border ?? BorderType.Single;
    this.borderColor = options.borderColor ?? "#ffffff";
    this.title = options.title;
    this.background = options.background ?? false;

    this.margin = 1;
  }

  public override populateLayout(container: Node): void {
    super.populateLayout(container);

    if (this.title) {
      const titleContainer = Yoga.Node.create();
      titleContainer.setPositionType(PositionType.Absolute);
      this.title.populateLayout(titleContainer);
      container.insertChild(titleContainer, container.getChildCount());
    }
  }

  public render(): RenderItem[] {
    if (!this.yogaNode) {
      return [];
    }

    const renderItems = super.render();
    renderItems.push({
      type: 'box',
      borderType: this.borderType,
      color: this.borderColor,
      zIndex: this.zIndex - 0.001,
      geometry: {
        left: this.yogaNode.getComputedLeft() - 1,
        top: this.yogaNode.getComputedTop() - 1,
        width: this.yogaNode.getComputedWidth() + 2,
        height: this.yogaNode.getComputedHeight() + 2,
      }
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
