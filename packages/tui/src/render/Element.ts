import EventEmitter from "events";
import * as R from "radash";
import Yoga, { Display, Node as YogaNode } from "yoga-layout";
import { createLogger } from "../utils/logger.js";
import { Document } from "./Document.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { RenderItem } from "./RenderItem.js";
import { Styles } from "./Styles.js";
import { Window } from "./Window.js";

const logger = createLogger("Element");

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface ElementEvents {}

export class Element {
  protected yogaNode?: YogaNode;
  private _document: Document;
  private parent?: Element;
  public id: string | undefined;
  public readonly style = new Styles();
  private _children: Element[] = [];
  public zIndex = 0;
  public tabIndex: number | undefined;
  public focused = false;
  protected _computedWidth = 0;
  protected _computedHeight = 0;
  protected eventEmitter = new EventEmitter();
  private elementDidMountCalled = false;
  private elementWillUnmountCalled = false;

  public constructor(document: Document) {
    this._document = document;
  }

  public populateLayout(container: YogaNode): void {
    this.yogaNode = Yoga.Node.create();
    this.style.apply(this.yogaNode);
    for (const child of this._children) {
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
    for (const child of this._children) {
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

  public handleKeyDown(event: KeyboardEvent): void {
    this.activeElement?.onKeyDown(event);
  }

  protected onKeyDown(event: KeyboardEvent): void {
    this.parent?.onKeyDown(event);
  }

  protected tab(offset: number): void {
    let currentTabElement: Element | undefined;
    const tabElements: Element[] = [];
    this.walkChildrenDeep((child) => {
      if (child.tabIndex !== undefined) {
        tabElements.push(child);
      }
      if (child.focused) {
        if (currentTabElement) {
          logger.warn("multiple focused elements");
        } else {
          currentTabElement = child;
        }
      }
      child.focused = false;
    });
    if (tabElements.length > 0) {
      const sortedTabElements = R.sort(tabElements, (c) => c.tabIndex ?? 0);
      if (currentTabElement) {
        const currentTabElementIndex = sortedTabElements.indexOf(currentTabElement);
        let nextIndex = currentTabElementIndex + offset;
        while (nextIndex < 0) {
          nextIndex += sortedTabElements.length;
        }
        nextIndex = nextIndex % sortedTabElements.length;
        sortedTabElements[nextIndex].focused = true;
      } else {
        sortedTabElements[0].focused = true;
      }
    }
  }

  public get activeElement(): Element | null {
    let currentTabElement: Element | null = null;
    this.walkChildrenDeep((child) => {
      if (child.focused) {
        if (currentTabElement) {
          logger.warn("multiple focused elements");
        } else {
          currentTabElement = child;
        }
      }
    });
    return currentTabElement;
  }

  protected walkChildrenDeep(fn: (child: Element) => void): void {
    for (const child of this._children) {
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

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  public closest<T extends Element>(t: new (...args: any[]) => T): T | undefined {
    if (this instanceof t) {
      return this;
    }
    return this.parent?.closest(t);
  }

  public get document(): Document {
    return this._document;
  }

  public get window(): Window {
    return this._document.window;
  }

  public focus(): void {
    this.window.walkChildrenDeep((child) => {
      if (child === this) {
        child.focused = true;
      } else {
        child.focused = false;
      }
    });
  }

  public get isMounted(): boolean {
    if (!this.parent) {
      return false;
    }
    return this.parent.isMounted;
  }

  public appendChild(child: Element): void {
    child.parent = this;
    this._children.push(child);
    if (this.isMounted) {
      if (!this.elementDidMountCalled) {
        child.elementDidMount();
        this.elementDidMountCalled = true;
        this.elementWillUnmountCalled = false;
      }
      child.walkChildrenDeep((c) => {
        if (!c.elementDidMountCalled) {
          c.elementDidMount();
          c.elementDidMountCalled = true;
          c.elementWillUnmountCalled = false;
        }
      });
    }
  }

  public removeChild(child: Element): void {
    if (!this.elementWillUnmountCalled) {
      child.elementWillUnmount();
      this.elementDidMountCalled = false;
      this.elementWillUnmountCalled = true;
    }
    child.walkChildrenDeep((c) => {
      if (!c.elementWillUnmountCalled) {
        c.elementWillUnmount();
        c.elementDidMountCalled = false;
        c.elementWillUnmountCalled = true;
      }
    });
    const i = this._children.indexOf(child);
    if (i >= 0) {
      this._children.splice(i, 1);
    }
  }

  protected elementDidMount(): void {}

  protected elementWillUnmount(): void {}

  public get firstElementChild(): Element | null {
    return this._children[0] ?? null;
  }

  public get lastElementChild(): Element | null {
    return this._children[this._children.length - 1] ?? null;
  }

  public get childElementCount(): number {
    return this._children.length;
  }

  public get children(): ReadonlyArray<Element> {
    return this._children;
  }

  public get parentElement(): Element | null {
    return this.parent ?? null;
  }
}
