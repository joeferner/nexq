import * as R from "radash";
import { Key } from "readline";
import Yoga, { Display, Node as YogaNode } from "yoga-layout";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { RenderItem } from "./RenderItem.js";
import { Styles } from "./Styles.js";

const logger = createLogger("Component");

export class Component {
  protected yogaNode?: YogaNode;
  public id: string | undefined;
  public readonly style = new Styles();
  public children: Component[] = [];
  public zIndex = 0;
  public tabIndex: number | undefined;
  public focused = false;
  protected _computedWidth = 0;
  protected _computedHeight = 0;

  public populateLayout(container: YogaNode): void {
    this.yogaNode = Yoga.Node.create();
    this.style.apply(this.yogaNode);
    for (const child of this.children) {
      child.populateLayout(this.yogaNode);
    }
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public render(): RenderItem[] {
    if (!this.yogaNode) {
      return [];
    }

    if (this.style.display === Display.None) {
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
