import { Display, FlexDirection, Justify, PositionType, Node as YogaNode } from "yoga-layout/load";
import { isInputMatch } from "../utils/input.js";
import { Box } from "./Box.js";
import { Document } from "./Document.js";
import { Element } from "./Element.js";
import { Geometry } from "./Geometry.js";
import { KeyboardEvent } from "./KeyboardEvent.js";
import { RenderItem } from "./RenderItem.js";
import { fgColor } from "./color.js";

export interface UpdateOptions {
  title: string;
  children: Element[];
}

export abstract class Dialog<TShowOptions, TResults> extends Element {
  protected box: Box;
  private lastParentYogaNode?: YogaNode;
  protected options?: TShowOptions;
  private resolve?: (value: TResults | undefined) => unknown;
  private lastFocus?: Element;
  public titleColor: string;

  protected constructor(document: Document) {
    super(document);

    this.zIndex = 100;
    this.style.positionType = PositionType.Absolute;
    this.style.display = Display.None;

    this.titleColor = "#ffffff";

    this.box = new Box(document);
    this.box.borderColor = "#ffffff";
    this.box.style.flexDirection = FlexDirection.Column;
    this.box.style.justifyContent = Justify.Center;
    this.box.style.paddingLeft = 2;
    this.box.style.paddingRight = 2;
    this.box.style.paddingTop = 1;
    this.box.style.paddingBottom = 1;
    this.appendChild(this.box);
  }

  public set title(title: string) {
    this.box.title = fgColor(this.titleColor)`<${title}>`;
  }

  public set borderColor(borderColor: string) {
    this.box.borderColor = borderColor;
  }

  public async show(options: TShowOptions): Promise<TResults | undefined> {
    this.style.display = Display.Flex;
    this.options = options;

    this.lastFocus = this.document.activeElement ?? undefined;
    await this.onShow(options);
    await this.window.refresh();

    return new Promise<TResults | undefined>((resolve) => {
      this.resolve = resolve;
    });
  }

  protected async onShow(_options: TShowOptions): Promise<void> {}

  public close(result: TResults | undefined): void {
    this.style.display = Display.None;
    this.lastFocus?.focus();
    this.resolve?.(result);
    this.resolve = undefined;
    this.options = undefined;
    this.lastFocus = undefined;
    void this.window.refresh();
  }

  public override populateLayout(node: YogaNode): void {
    this.lastParentYogaNode = node;
    super.populateLayout(node);
  }

  public override render(container: Geometry): RenderItem[] {
    if (!this.lastParentYogaNode) {
      return super.render(container);
    }
    const renderItems = super.render(container);
    const left = Math.floor((this.lastParentYogaNode.getComputedWidth() - this.box.computedWidth) / 2);
    const top = Math.floor((this.lastParentYogaNode.getComputedHeight() - this.box.computedHeight) / 2);
    for (const renderItem of renderItems) {
      if ("container" in renderItem) {
        renderItem.container = {
          ...renderItem.container,
          top: renderItem.container.top + top,
          left: renderItem.container.left + left,
        };
      }
      renderItem.geometry = {
        ...renderItem.geometry,
        top: renderItem.geometry.top + top,
        left: renderItem.geometry.left + left,
      };
    }
    return renderItems;
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "escape")) {
      this.close(undefined);
      return;
    }

    if (isInputMatch(event, "left") || isInputMatch(event, "shift-tab")) {
      this.tab(-1);
      return;
    }

    if (isInputMatch(event, "right") || isInputMatch(event, "tab")) {
      this.tab(1);
      return;
    }

    super.onKeyDown(event);
  }
}
