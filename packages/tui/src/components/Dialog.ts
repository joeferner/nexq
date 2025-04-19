import { Key } from "readline";
import { FlexDirection } from "yoga-layout";
import { Node as YogaNode } from "yoga-layout/load";
import { NexqState } from "../NexqState.js";
import { BoxBorder, BoxComponent } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { RenderItem } from "../render/RenderItem.js";
import { TextComponent } from "../render/TextComponent.js";

export interface UpdateOptions {
  title: string;
  children: Component[];
}

export abstract class Dialog<TShowOptions, TResults> extends Component {
  private _children: Component[];
  private box: BoxComponent;
  private lastParentYogaNode?: YogaNode;
  protected options?: TShowOptions;
  private resolve?: (value: TResults) => unknown;

  protected constructor(
    protected readonly state: NexqState,
    protected readonly id: string
  ) {
    super();
    this.box = new BoxComponent({
      children: [],
      direction: FlexDirection.Column,
    });
    this._children = [this.box];

    state.on("keypress", (chunk, key) => {
      if (!this.isFocused) {
        return;
      }
      this.handleKeyPress(chunk, key);
    });
  }

  protected handleKeyPress(_chunk: string, _key: Key | undefined): void { }

  public update(options: UpdateOptions): void {
    this.box = new BoxComponent({
      children: options.children,
      direction: FlexDirection.Column,
      title: new BoxComponent({
        direction: FlexDirection.Row,
        children: [new TextComponent({ text: ` ${options.title} ` })],
      }),
      border: BoxBorder.Single,
      borderColor: this.state.dialogBorderColor,
      background: true,
      zIndex: 100,
    });
    this._children = [this.box];
    this.state.emit("changed");
  }

  public get children(): Component[] {
    return this._children;
  }

  public show(options: TShowOptions): Promise<TResults> {
    this.options = options;

    return new Promise<TResults>((resolve) => {
      this.resolve = resolve;
      this.state.pushFocus(this.id);
      this.refresh();
      this.state.emit("changed");
    });
  }

  protected abstract refresh(): void;

  public close(result: TResults): void {
    if (!this.isFocused) {
      throw new Error("invalid state");
    }
    this.state.popFocus();
    this.resolve?.(result);
    this.resolve = undefined;
    this.options = undefined;
    this._children = [];
    this.state.emit("changed");
  }

  public get isFocused(): boolean {
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
      renderItem.geometry.top += Math.floor((this.lastParentYogaNode.getComputedHeight() - this.box.computedHeight) / 2);
    }
    return renderItems;
  }
}
