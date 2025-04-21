import { Display, FlexDirection, Justify, PositionType, Node as YogaNode } from "yoga-layout/load";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { RenderItem } from "../render/RenderItem.js";
import { Box } from "./Box.js";
import { Text } from "./Text.js";

export interface UpdateOptions {
  title: string;
  children: Component[];
}

export abstract class Dialog<TShowOptions, TResults> extends Component {
  protected box: Box;
  private lastParentYogaNode?: YogaNode;
  protected options?: TShowOptions;
  private resolve?: (value: TResults) => unknown;

  protected constructor(
    protected readonly state: NexqState,
    id: string
  ) {
    super();

    this.id = id;
    this.zIndex = 100;
    this.style.positionType = PositionType.Absolute;
    this.style.display = Display.None;

    this.box = new Box();
    this.box.borderColor = state.borderColor;
    this.box.style.flexDirection = FlexDirection.Column;
    this.box.style.justifyContent = Justify.Center;
    this.box.style.paddingLeft = 2;
    this.box.style.paddingRight = 2;
    this.box.style.paddingTop = 1;
    this.box.style.paddingBottom = 1;
    this.children.push(this.box);

    state.on("keypress", (chunk, key) => {
      if (!this.isFocused) {
        return;
      }
      this.handleKeyPress(chunk, key);
    });
  }

  public set title(title: string) {
    this.box.title = new Text({ text: ` ${title} ` });
  }

  public show(options: TShowOptions): Promise<TResults> {
    this.style.display = Display.Flex;
    this.options = options;

    return new Promise<TResults>((resolve) => {
      this.resolve = resolve;
      if (this.id) {
        this.state.pushFocus(this.id);
      }
      this.state.emit("changed");
    });
  }

  public close(result: TResults): void {
    if (!this.isFocused) {
      throw new Error("invalid state");
    }
    this.style.display = Display.None;
    this.state.popFocus();
    this.resolve?.(result);
    this.resolve = undefined;
    this.options = undefined;
    this.state.emit("changed");
  }

  public get isFocused(): boolean {
    if (!this.id) {
      return false;
    }
    return this.state.focus.startsWith(this.id);
  }

  public override populateLayout(node: YogaNode): void {
    this.lastParentYogaNode = node;
    super.populateLayout(node);
  }

  public override render(): RenderItem[] {
    if (!this.lastParentYogaNode) {
      return super.render();
    }
    const renderItems = super.render();
    for (const renderItem of renderItems) {
      renderItem.geometry.left += Math.floor((this.lastParentYogaNode.getComputedWidth() - this.box.computedWidth) / 2);
      renderItem.geometry.top += Math.floor(
        (this.lastParentYogaNode.getComputedHeight() - this.box.computedHeight) / 2
      );
    }
    return renderItems;
  }
}
