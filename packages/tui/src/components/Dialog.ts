import { Key } from "readline";
import { NexqState } from "../NexqState.js";
import { BoxBorder, BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { RenderItem } from "../render/RenderItem.js";
import { TextComponent } from "../render/TextComponent.js";

export interface UpdateOptions {
  title: string;
  children: Component[];
}

export abstract class Dialog<TShowOptions, TResults> extends Component {
  private _children: Component[];
  private box: BoxComponent;
  private lastContainer?: Geometry;
  protected options?: TShowOptions;
  private resolve?: (value: TResults) => unknown;

  protected constructor(
    protected readonly state: NexqState,
    protected readonly id: string
  ) {
    super();
    this.box = new BoxComponent({
      children: [],
      direction: BoxDirection.Vertical,
    });
    this._children = [this.box];

    state.on("keypress", (chunk, key) => {
      if (!this.isFocused) {
        return;
      }
      this.handleKeyPress(chunk, key);
    });
  }

  protected handleKeyPress(_chunk: string, _key: Key | undefined): void {}

  public update(options: UpdateOptions): void {
    this.box = new BoxComponent({
      children: options.children,
      direction: BoxDirection.Vertical,
      title: new BoxComponent({
        direction: BoxDirection.Horizontal,
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

  public override calculateGeometry(container: Geometry): void {
    this.lastContainer = { ...container };
    super.calculateGeometry(container);
  }

  public override render(): RenderItem[] {
    if (!this.lastContainer) {
      return super.render();
    }
    const renderItems = super.render();
    for (const renderItem of renderItems) {
      renderItem.geometry.left += Math.floor((this.lastContainer.width - this.box.geometry.width) / 2);
      renderItem.geometry.top += Math.floor((this.lastContainer.height - this.box.geometry.height) / 2);
    }
    return renderItems;
  }

  public override get geometry(): Geometry {
    return { left: 0, top: 0, height: 0, width: 0 };
  }
}
