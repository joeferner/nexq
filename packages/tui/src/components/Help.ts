import { FlexDirection, Wrap } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { isPathMatch } from "../render/RouterElement.js";
import { Text } from "../render/Text.js";
import { DescribeQueue } from "./DescribeQueue.js";
import { QueueMessages } from "./QueueMessages.js";
import { Queues } from "./Queues.js";
import { Topics } from "./Topics.js";

export interface HelpItem {
  id: string;
  name: string;
  shortcut: string;
}

export class Help extends Element {
  private lastFocus?: string;
  private readonly boundRefreshChildren = this.refreshChildren.bind(this);

  public constructor(document: Document) {
    super(document);
    this.id = "Help";
    this.style.flexDirection = FlexDirection.Column;
    this.style.flexWrap = Wrap.Wrap;
    this.style.columnGap = 3;
    this.style.maxHeight = 4;
  }

  protected override elementDidMount(): void {
    this.refreshChildren();
    this.window.addEventListener("popstate", this.boundRefreshChildren);
    this.window.addEventListener("pushstate", this.boundRefreshChildren);
  }

  protected override elementWillUnmount(): void {
    this.window.removeEventListener("popstate", this.boundRefreshChildren);
    this.window.removeEventListener("pushstate", this.boundRefreshChildren);
  }

  private refreshChildren(): void {
    const pathname = this.window.location.pathname;
    if (this.lastFocus === pathname) {
      return;
    }

    let helpItems: HelpItem[];
    if (isPathMatch(DescribeQueue.PATH, pathname)) {
      helpItems = DescribeQueue.HELP_ITEMS;
    } else if (isPathMatch(QueueMessages.PATH, pathname)) {
      helpItems = QueueMessages.HELP_ITEMS;
    } else if (isPathMatch(Queues.PATH, pathname)) {
      helpItems = Queues.HELP_ITEMS;
    } else if (isPathMatch(Topics.PATH, pathname)) {
      helpItems = Topics.HELP_ITEMS;
    } else {
      helpItems = [];
    }
    const maxShortcutWidth = helpItems.reduce((p, h) => Math.max(p, h.shortcut.length), 0);

    while (this.lastElementChild) {
      this.removeChild(this.lastElementChild);
    }

    for (const helpItem of helpItems) {
      const padding = " ".repeat(maxShortcutWidth - helpItem.shortcut.length);

      const row = new DivElement(this.document);
      row.style.flexDirection = FlexDirection.Row;
      row.appendChild(
        new Text(this.document, { text: `<${helpItem.shortcut}>${padding} `, color: NexqStyles.helpHotkeyColor })
      );
      row.appendChild(new Text(this.document, { text: helpItem.name, color: NexqStyles.helpNameColor }));
      this.appendChild(row);
    }
    this.lastFocus = pathname;
  }
}
