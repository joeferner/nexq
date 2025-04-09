import { Key } from "readline";
import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { TextComponent } from "../render/TextComponent.js";
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

export class TableView<T> extends Component {
  private readonly box = new BoxComponent({ children: [], direction: BoxDirection.Vertical });
  private readonly _children: Component[] = [this.box];
  private _items: T[] = [];
  public selectedIndex = 0;
  public offset = 0;
  public _columns: TableViewColumn<T>[];
  private readonly columnWidths: number[] = [];
  public itemTextColor: string;
  public sortedColumnIndex = 0;
  public sortedColumnDirection = SortDirection.Ascending;

  public constructor(options: TableViewOptions<T>) {
    super();
    this._columns = options.columns;
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

  public get children(): Component[] {
    return this._children;
  }

  public get columns(): TableViewColumn<T>[] {
    return this._columns;
  }

  public override calculateGeometry(container: Geometry): void {
    if (this._children.length !== container.height) {
      const children: TextComponent[] = [];
      for (let i = 0; i < container.height; i++) {
        children.push(new TextComponent({ text: "", color: this.itemTextColor }));
      }
      this.box.children = children;
    }

    const children = this.box.children as TextComponent[];

    this.selectedIndex = Math.max(0, Math.min(this.selectedIndex, this.items.length - 1));
    if (this.selectedIndex < this.offset) {
      this.offset = this.selectedIndex;
    } else if (this.selectedIndex >= this.offset + container.height - 1) {
      this.offset = this.selectedIndex - container.height + 2;
    }

    for (let i = 0; i < container.height - 1; i++) {
      const index = this.offset + i;
      children[i + 1].inverse = this.selectedIndex === index;
      const item = this.items[index];
      children[i + 1].text = item ? this.createRowText(item, container.width) : "";
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

    super.calculateGeometry(container);
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
    return text;
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

  public handleKeyPress(key: Key | undefined): boolean {
    if (isInputMatch(key, "down")) {
      this.selectedIndex++;
      return true;
    }

    if (isInputMatch(key, "up")) {
      this.selectedIndex--;
      return true;
    }

    if (isInputMatch(key, "pagedown")) {
      this.selectedIndex += this.geometry.height - 3;
      return true;
    }

    if (isInputMatch(key, "pageup")) {
      this.selectedIndex -= this.geometry.height - 3;
      return true;
    }

    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      if (isInputMatch(key, column.sortKeyboardShortcut)) {
        this.sortedColumnDirection =
          this.sortedColumnIndex === i ? flipSortDirection(this.sortedColumnDirection) : SortDirection.Ascending;
        this.sortedColumnIndex = i;
        this.sortItems();
        this.updateColumnWidths();
        return true;
      }
    }

    return false;
  }

  private sortItems(): void {
    const sort = this.columns[this.sortedColumnIndex].sortItems ?? ((items: T[]): T[] => items);
    this._items = sort(this.items, this.sortedColumnDirection);
  }
}
