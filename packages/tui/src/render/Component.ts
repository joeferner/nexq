import * as R from "radash";
import { Key } from "readline";
import Yoga, { Align, Display, Edge, FlexDirection, Justify, PositionType, Node as YogaNode } from "yoga-layout";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { RenderItem } from "./RenderItem.js";

const logger = createLogger("Component");

export class Component {
  protected yogaNode?: YogaNode;
  public id: string | undefined;
  public height: number | "auto" | `${number}%` | undefined;
  public width: number | "auto" | `${number}%` | undefined;
  public flexGrow: number | undefined;
  public alignItems = Align.FlexStart;
  public flexDirection = FlexDirection.Row;
  public justifyContent = Justify.FlexStart;
  public positionType = PositionType.Relative;
  public display = Display.Flex;
  public margin?: {
    left?: number | "auto" | `${number}%` | undefined;
    right?: number | "auto" | `${number}%` | undefined;
    top?: number | "auto" | `${number}%` | undefined;
    bottom?: number | "auto" | `${number}%` | undefined;
  };
  public padding?: {
    left?: number | `${number}%` | undefined;
    right?: number | `${number}%` | undefined;
    top?: number | `${number}%` | undefined;
    bottom?: number | `${number}%` | undefined;
  };
  public children: Component[] = [];
  public zIndex = 0;
  public tabIndex: number | undefined;
  public focused = false;
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
    this.yogaNode.setPositionType(this.positionType);
    this.yogaNode.setMargin(Edge.Left, this.margin?.left);
    this.yogaNode.setMargin(Edge.Right, this.margin?.right);
    this.yogaNode.setMargin(Edge.Top, this.margin?.top);
    this.yogaNode.setMargin(Edge.Bottom, this.margin?.bottom);
    this.yogaNode.setPadding(Edge.Left, this.padding?.left);
    this.yogaNode.setPadding(Edge.Right, this.padding?.right);
    this.yogaNode.setPadding(Edge.Top, this.padding?.top);
    this.yogaNode.setPadding(Edge.Bottom, this.padding?.bottom);
    this.yogaNode.setDisplay(this.display);
    for (const child of this.children) {
      child.populateLayout(this.yogaNode);
    }
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public render(): RenderItem[] {
    if (!this.yogaNode) {
      return [];
    }

    if (this.display === Display.None) {
      return [];
    }

    this._computedWidth = this.yogaNode.getComputedWidth();
    this._computedHeight = this.yogaNode.getComputedHeight();

    const renderItems: RenderItem[] = this.preRender(this.yogaNode);
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

  protected handleKeyPress(_chunk: string, key: Key | undefined): void {
    if (isInputMatch(key, "left") || isInputMatch(key, "shift-tab")) {
      this.tab(-1);
    } else if (isInputMatch(key, "right") || isInputMatch(key, "tab")) {
      this.tab(1);
    } else {
      logger.debug("unhandled key press", JSON.stringify(key));
    }
  }

  protected tab(offset: number): void {
    let currentTabComponent: Component | undefined;
    const tabComponents: Component[] = [];
    this.walkChildrenDeep((child) => {
      if (child.tabIndex !== undefined) {
        tabComponents.push(child);
      }
      if (child.focused) {
        if (currentTabComponent) {
          logger.warn("multiple focused components");
        } else {
          currentTabComponent = child;
        }
      }
      child.focused = false;
    });
    if (tabComponents.length > 0) {
      const sortedTabComponents = R.sort(tabComponents, (c) => c.tabIndex ?? 0);
      if (currentTabComponent) {
        const currentTabComponentIndex = sortedTabComponents.indexOf(currentTabComponent);
        let nextIndex = currentTabComponentIndex + offset;
        while (nextIndex < 0) {
          nextIndex += sortedTabComponents.length;
        }
        nextIndex = nextIndex % sortedTabComponents.length;
        sortedTabComponents[nextIndex].focused = true;
      } else {
        sortedTabComponents[0].focused = true;
      }
    }
  }

  public setFocusedChild(focusChild: Component): void {
    this.walkChildrenDeep((child) => {
      if (child === focusChild) {
        child.focused = true;
      } else {
        child.focused = false;
      }
    });
  }

  public getFocusedChild(): Component | undefined {
    let currentTabComponent: Component | undefined;
    this.walkChildrenDeep((child) => {
      if (child.focused) {
        if (currentTabComponent) {
          logger.warn("multiple focused components");
        } else {
          currentTabComponent = child;
        }
      }
    });
    return currentTabComponent;
  }

  protected walkChildrenDeep(fn: (child: Component) => void): void {
    for (const child of this.children) {
      fn(child);
      child.walkChildrenDeep(fn);
    }
  }

  protected preRender(_yogaNode: YogaNode): RenderItem[] {
    return [];
  }

  public get computedWidth(): number {
    return this._computedWidth;
  }

  public get computedHeight(): number {
    return this._computedHeight;
  }
}
