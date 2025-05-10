import EventEmitter from "events";
import * as R from "radash";
import Yoga, { Display, Overflow, Node as YogaNode } from "yoga-layout";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { Document } from "./Document.js";
import { Geometry, geometryFromYogaNode } from "./Geometry.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { BoxRenderItem, RenderItemWithChildren } from "./RenderItem.js";
import { Style } from "./Style.js";
import { Window } from "./Window.js";

const logger = createLogger("Element");

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface ElementEvents {}

export abstract class Element {
  protected yogaNode?: YogaNode;
  private _document: Document;
  private parent?: Element;
  public id: string | undefined;
  public readonly _style;
  private _children: Element[] = [];
  public zIndex = 0;
  public tabIndex: number | undefined;
  public focused = false;
  protected _clientWidth = 0;
  protected _clientHeight = 0;
  protected _offsetWidth = 0;
  protected _offsetHeight = 0;
  protected _scrollWidth = 0;
  protected _scrollHeight = 0;
  protected eventEmitter = new EventEmitter();
  private elementDidMountCalled = false;
  private elementWillUnmountCalled = false;
  private _scrollTop = 0;
  private _scrollLeft = 0;

  protected constructor(document: Document) {
    this._document = document;
    this._style = this.createStyle();
  }

  public get style(): Style {
    return this._style;
  }

  protected createStyle(): Style {
    return new Style();
  }

  public populateLayout(container: YogaNode): void {
    this.yogaNode = Yoga.Node.create();
    this.style.apply(this.yogaNode);
    for (const child of this._children) {
      child.populateLayout(this.yogaNode);
    }
    container.insertChild(this.yogaNode, container.getChildCount());
  }

  public render(options: RenderOptions): void {
    if (!this.yogaNode) {
      return;
    }

    if (this.style.display === Display.None) {
      return;
    }

    const geometry = geometryFromYogaNode(this.yogaNode);

    this._clientWidth = geometry.width - this.borderWidthLeft - this.borderWidthRight;
    this._clientHeight = geometry.height - this.borderWidthTop - this.borderWidthBottom;
    this._offsetWidth = geometry.width;
    this._offsetHeight = geometry.height;
    this._scrollWidth = this.children.reduce((p, c) => p + c.offsetWidth, 0);
    this._scrollHeight = this.children.reduce((p, c) => p + c.offsetHeight, 0);

    const box: BoxRenderItem = {
      name: this.id,
      type: "box",
      children: [],
      zIndex: this.zIndex,
      geometry: geometry,
    };
    options.parent.children.push(box);

    this.renderChildren({
      ...options,
      parent: box,
      container: geometry,
    });
  }

  protected renderChildren(options: RenderChildrenOptions): void {
    for (const child of this._children) {
      child.render(options);
    }
  }

  public handleKeyDown(event: KeyboardEvent): void {
    this.activeElement?.onKeyDown(event);
  }

  public scrollTo(xCoord: number, yCoord: number): void {
    this._scrollLeft = Math.max(0, Math.min(xCoord, this.scrollWidth - this.clientWidth));
    this._scrollTop = Math.max(0, Math.min(yCoord, this.scrollHeight - this.clientHeight));
  }

  protected onKeyDown(event: KeyboardEvent): void {
    if (this.style.overflowY === Overflow.Scroll) {
      if (isInputMatch(event, "down")) {
        this.scrollTo(this.scrollLeft, this.scrollTop + 1);
        return;
      }

      if (isInputMatch(event, "up")) {
        this.scrollTo(this.scrollLeft, this.scrollTop - 1);
        return;
      }

      if (isInputMatch(event, "pagedown")) {
        this.scrollTo(0, this.scrollTop + this.clientHeight);
        return;
      }

      if (isInputMatch(event, "pageup")) {
        this.scrollTo(0, this.scrollTop - this.clientHeight);
        return;
      }
    }

    if (this.style.overflowX === Overflow.Scroll) {
      if (isInputMatch(event, "left")) {
        this.scrollTo(this.scrollLeft - 1, this.scrollTop);
        return;
      }

      if (isInputMatch(event, "right")) {
        this.scrollTo(this.scrollLeft + 1, this.scrollTop);
        return;
      }
    }

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
        child.onBlur();
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
        if (!sortedTabElements[nextIndex].focused) {
          sortedTabElements[nextIndex].focused = true;
          sortedTabElements[nextIndex].onFocus();
        }
      } else {
        if (!sortedTabElements[0].focused) {
          sortedTabElements[0].focused = true;
          sortedTabElements[0].onFocus();
        }
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

  /**
   * returns the inner width of an element in pixels, including padding but
   * not the horizontal scrollbar width, border, or margin
   */
  public get clientWidth(): number {
    return this._clientWidth;
  }

  /**
   * returns the inner height of an element in pixels, including padding but
   * not the horizontal scrollbar height, border, or margin
   */
  public get clientHeight(): number {
    return this._clientHeight;
  }

  /**
   * is a measurement which includes the element borders, the element horizontal
   * padding, the element vertical scrollbar (if present, if rendered) and
   * the element CSS width.
   */
  public get offsetWidth(): number {
    return this._offsetWidth;
  }

  /**
   * is a measurement which includes the element borders, the element vertical
   * padding, the element horizontal scrollbar (if present, if rendered) and
   * the element CSS height.
   */
  public get offsetHeight(): number {
    return this._offsetHeight;
  }

  /**
   * is a measurement of the width of an element's content including content
   * not visible on the screen due to overflow
   */
  public get scrollWidth(): number {
    return this._scrollWidth;
  }

  /**
   * is a measurement of the height of an element's content including content
   * not visible on the screen due to overflow
   */
  public get scrollHeight(): number {
    return this._scrollHeight;
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

  public get scrollTop(): number {
    return this._scrollTop;
  }

  public set scrollTop(scrollTop: number) {
    this._scrollTop = scrollTop;
  }

  public get scrollLeft(): number {
    return this._scrollLeft;
  }

  public set scrollLeft(scrollLeft: number) {
    this._scrollLeft = scrollLeft;
  }

  public focus(): void {
    this.window.walkChildrenDeep((child) => {
      if (child === this) {
        if (!child.focused) {
          child.focused = true;
          this.onFocus();
        }
      } else {
        if (child.focused) {
          child.focused = false;
          child.onBlur();
        }
      }
    });
  }

  protected onFocus(): void {}

  protected onBlur(): void {}

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
        this.elementDidMountCalled = true;
        this.elementWillUnmountCalled = false;
        child.elementDidMount();
      }
      child.walkChildrenDeep((c) => {
        if (!c.elementDidMountCalled) {
          c.elementDidMountCalled = true;
          c.elementWillUnmountCalled = false;
          c.elementDidMount();
        }
      });
    }
  }

  public removeChild(child: Element): void {
    if (!this.elementWillUnmountCalled) {
      this.elementDidMountCalled = false;
      this.elementWillUnmountCalled = true;
      child.elementWillUnmount();
    }
    child.walkChildrenDeep((c) => {
      if (!c.elementWillUnmountCalled) {
        c.elementDidMountCalled = false;
        c.elementWillUnmountCalled = true;
        c.elementWillUnmount();
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

  private get borderWidthLeft(): number {
    return borderWidth(this.style.borderLeftStyle);
  }

  private get borderWidthRight(): number {
    return borderWidth(this.style.borderRightStyle);
  }

  private get borderWidthTop(): number {
    return borderWidth(this.style.borderTopStyle);
  }

  private get borderWidthBottom(): number {
    return borderWidth(this.style.borderBottomStyle);
  }
}

export interface RenderOptions {
  root: RenderItemWithChildren;
  parent: RenderItemWithChildren;
}

export interface RenderChildrenOptions extends RenderOptions {
  container: Geometry;
}

function borderWidth(borderStyle: string | undefined): number {
  return borderStyle === undefined || borderStyle === "none" ? 0 : 1;
}
