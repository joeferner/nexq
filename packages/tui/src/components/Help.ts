import { Screen, NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { TextComponent } from "../render/TextComponent.js";
import { Queues } from "./Queues.js";

export interface HelpItem {
  id: string;
  name: string;
  shortcut: string;
}

export class Help extends Component {
  private readonly box: BoxComponent;
  private readonly _children: Component[];
  private lastFocus?: Screen;

  public constructor(private readonly state: NexqState) {
    super();
    this.box = new BoxComponent({
      children: [],
      direction: BoxDirection.Vertical,
    });
    this._children = [this.box];
    this.refreshChildren();
    state.on("changed", () => {
      this.refreshChildren();
    });
  }

  private refreshChildren(): void {
    if (this.lastFocus === this.state.screen) {
      return;
    }

    let helpItems: HelpItem[];
    if (this.state.screen === Screen.Queues) {
      helpItems = Queues.HELP_ITEMS;
    } else {
      helpItems = [];
    }
    const newChildren: Component[] = [];
    const maxShortcutWidth = helpItems.reduce((p, h) => Math.max(p, h.shortcut.length), 0);
    for (const helpItem of helpItems) {
      const padding = " ".repeat(maxShortcutWidth - helpItem.shortcut.length);
      newChildren.push(
        new BoxComponent({
          direction: BoxDirection.Horizontal,
          children: [
            new TextComponent({ text: `<${helpItem.shortcut}>${padding} `, color: this.state.helpHotkeyColor }),
            new TextComponent({ text: helpItem.name, color: this.state.helpNameColor }),
          ],
        })
      );
    }
    this.box.children = newChildren;
    this.lastFocus = this.state.screen;
  }

  public get children(): Component[] {
    return this._children;
  }

  public override calculateGeometry(container: Geometry): void {
    super.calculateGeometry(container);
  }
}
