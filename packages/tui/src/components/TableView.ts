import { FlexDirection, Node as YogaNode } from "yoga-layout";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { Text } from "../render/Text.js";
import { isInputMatch } from "../utils/input.js";

export interface TableViewOptions<T> {
  columns: TableViewColumn<T>[];
  itemTextColor?: string;
}

export enum SortDirection {
  Ascending = "asc",
  Descending = "desc",
}

export function flipSortDirection(dir: SortDirection): SortDirection {
  return dir === SortDirection.Ascending ? SortDirection.Descending : SortDirection.Ascending;
}

export interface TableViewColumn<T> {
  title: string;
  align: "left" | "right";
  sortKeyboardShortcut: string;
  sortItems: (rows: T[], direction: SortDirection) => T[];
  render: (value: T) => string;
}

const COLUMN_MARGIN = 1;
const COLUMN_SORT_WIDTH = 1;

export class TableView<T> extends Element {
  private _items: T[] = [];
  public selectedIndex = 0;
  public offset = 0;
  public _columns: TableViewColumn<T>[];
  private readonly columnWidths: number[] = [];
  public itemTextColor: string;
  public sortedColumnIndex = 0;
  public sortedColumnDirection = SortDirection.Ascending;

  public constructor(document: Document, options: TableViewOptions<T>) {
    super(document);
    this._columns = options.columns;
    this.style.flexDirection = FlexDirection.Column;
    this.appendChild(new Text(document, { text: "Loading" }));
    this.itemTextColor = options.itemTextColor ?? "#ffffff";
    this.updateColumnWidths();
  }

  public set items(items: T[]) {
    this._items = items;
    this.updateColumnWidths();
    this.sortItems();
  }

  public get items(): T[] {
    return this._items;
  }

  public get columns(): TableViewColumn<T>[] {
    return this._columns;
  }

  public override populateLayout(container: YogaNode): void {
    const height = Math.max(1, this.computedHeight);
    const width = this.computedWidth;

    while (this.childElementCount > height && this.lastElementChild !== null) {
      this.removeChild(this.lastElementChild);
    }
    while (this.childElementCount < height) {
      this.appendChild(new Text(this.document, { text: "item", color: this.itemTextColor }));
    }

    this.selectedIndex = Math.max(0, Math.min(this.selectedIndex, this.items.length - 1));
    if (this.selectedIndex < this.offset) {
      this.offset = this.selectedIndex;
    } else if (this.selectedIndex >= this.offset + height - 1) {
      this.offset = this.selectedIndex - height + 2;
    }

    const children = this.children as Text[];
    for (let i = 0; i < height - 1; i++) {
      const child = children[i + 1];
      if (!child) {
        continue;
      }
      const index = this.offset + i;
      child.inverse = this.selectedIndex === index;
      const item = this.items[index];
      child.text = item ? this.createRowText(item, width) : "";
    }

    let header = "";
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      let columnTitle = column.title;
      if (this.sortedColumnIndex === i) {
        columnTitle += this.sortedColumnDirection === SortDirection.Ascending ? "↑" : "↓";
      }
      header += columnTitle;
      header += " ".repeat(Math.max(0, this.columnWidths[i] - columnTitle.length));
    }
    children[0].text = header;
    children[0].text.substring(0, width);

    super.populateLayout(container);
  }

  private createRowText(row: T, maxWidth: number): string {
    let text = "";
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      const columnText = column.render(row);
      const padding = " ".repeat(this.columnWidths[i] - columnText.length - 1);
      if (column.align === "left") {
        text += columnText + padding + " ";
      } else {
        text += padding + columnText + " ";
      }
    }
    text += " ".repeat(Math.max(0, maxWidth - text.length));
    return text.substring(0, maxWidth);
  }

  private updateColumnWidths(): void {
    this.columnWidths.length = 0;
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      this.columnWidths[i] = column.title.length + COLUMN_MARGIN;
      if (i === this.sortedColumnIndex) {
        this.columnWidths[i] += COLUMN_SORT_WIDTH;
      }
    }

    for (const item of this.items) {
      for (let i = 0; i < this.columns.length; i++) {
        const column = this.columns[i];
        const columnText = column.render(item);
        this.columnWidths[i] = Math.max(this.columnWidths[i] ?? 0, columnText.length + COLUMN_MARGIN);
      }
    }
  }

  public getSelectedItems(): T[] {
    const item = this.items[this.selectedIndex];
    if (item) {
      return [item];
    }
    return [];
  }

  public getCurrentItem(): T | undefined {
    return this.items[this.selectedIndex];
  }

  public override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "down")) {
      this.selectedIndex++;
      return;
    }

    if (isInputMatch(event, "up")) {
      this.selectedIndex--;
      return;
    }

    if (isInputMatch(event, "pagedown")) {
      this.selectedIndex += this.computedHeight - 3;
      return;
    }

    if (isInputMatch(event, "pageup")) {
      this.selectedIndex -= this.computedHeight - 3;
      return;
    }

    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      if (isInputMatch(event, column.sortKeyboardShortcut)) {
        this.sortedColumnDirection =
          this.sortedColumnIndex === i ? flipSortDirection(this.sortedColumnDirection) : SortDirection.Ascending;
        this.sortedColumnIndex = i;
        this.sortItems();
        this.updateColumnWidths();
        return;
      }
    }

    super.onKeyDown(event);
  }

  private sortItems(): void {
    const sort = this.columns[this.sortedColumnIndex].sortItems ?? ((items: T[]): T[] => items);
    this._items = sort(this.items, this.sortedColumnDirection);
  }
}
