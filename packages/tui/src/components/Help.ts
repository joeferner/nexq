import { FlexDirection } from "yoga-layout";
import { NexqState, Screen } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { Queues } from "./Queues.js";
import { Text } from "./Text.js";

export interface HelpItem {
  id: string;
  name: string;
  shortcut: string;
}

export class Help extends Component {
  private lastFocus?: Screen;

  public constructor(private readonly state: NexqState) {
    super();
    this.style.flexDirection = FlexDirection.Column;
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

      const row = new Component();
      row.style.flexDirection = FlexDirection.Row;
      row.children.push(new Text({ text: `<${helpItem.shortcut}>${padding} `, color: this.state.helpHotkeyColor }));
      row.children.push(new Text({ text: helpItem.name, color: this.state.helpNameColor }));
      newChildren.push(row);
    }
    this.children = newChildren;
    this.lastFocus = this.state.screen;
  }
}
