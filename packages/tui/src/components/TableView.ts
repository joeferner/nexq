import { FlexDirection, Overflow, Node as YogaNode } from "yoga-layout";
import { ansiLength, fgColor } from "../render/color.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { Style } from "../render/Style.js";
import { Text } from "../render/Text.js";
import { isInputMatch } from "../utils/input.js";
import { DivElement } from "../render/DivElement.js";

export interface TableViewOptions<T> {
  columns: TableViewColumn<T>[];
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

export class TableViewStyle extends Style {
  public itemTextColor = "#ffffff";
  public headerTextColor = "#ffffff";
  public sortTextColor = "#ffffff";
}

const COLUMN_MARGIN = 1;
const COLUMN_SORT_WIDTH = 1;

export class TableView<T> extends Element {
  private _items: T[] = [];
  public selectedIndex = 0;
  public _columns: TableViewColumn<T>[];
  private readonly columnWidths: number[] = [];
  public sortedColumnIndex = 0;
  public sortedColumnDirection = SortDirection.Ascending;
  private readonly header: Text;
  private readonly tableBody: DivElement;

  public constructor(document: Document, options: TableViewOptions<T>) {
    super(document);
    this._columns = options.columns;
    this.style.flexDirection = FlexDirection.Column;
    this.style.overflow = Overflow.Hidden;

    this.header = new Text(document, { text: "Loading" });
    this.appendChild(this.header);

    this.tableBody = new DivElement(document);
    this.tableBody.style.flexDirection = FlexDirection.Column;
    this.tableBody.style.overflow = Overflow.Scroll;
    this.appendChild(this.tableBody);

    this.updateColumnWidths();
  }

  protected override createStyle(): Style {
    return new TableViewStyle();
  }

  public override get style(): TableViewStyle {
    return super.style as TableViewStyle;
  }

  public set items(items: T[]) {
    this._items = items;
    while (this.tableBody.childElementCount > this.items.length && this.tableBody.lastElementChild) {
      this.tableBody.removeChild(this.tableBody.lastElementChild);
    }
    while (this.tableBody.childElementCount < this.items.length) {
      this.tableBody.appendChild(new Text(this.document, { text: "item", color: this.style.itemTextColor }));
    }
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
    const height = Math.max(1, this.clientHeight);
    const width = this.clientWidth;

    // ensure the selected item is scrolled into view
    this.selectedIndex = Math.max(0, Math.min(this.selectedIndex, this.items.length - 1));
    if (this.selectedIndex < this.tableBody.scrollTop) {
      this.tableBody.scrollTop = this.selectedIndex;
    } else if (this.selectedIndex >= this.tableBody.scrollTop + height - 1) {
      this.tableBody.scrollTop = this.selectedIndex - height + 2;
    }

    // update the child text to match the width of columns and element
    const tableBodyChildren = this.tableBody.children as Text[];
    for (let i = 0; i < this.items.length; i++) {
      const child = tableBodyChildren[i];
      if (!child) {
        continue;
      }
      child.style.inverse = this.selectedIndex === i;
      const item = this.items[i];
      child.text = item ? this.createRowText(item, width) : "";
    }

    // update header with sorting
    let header = "";
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      let columnTitle = column.title;
      if (this.sortedColumnIndex === i) {
        columnTitle += fgColor(
          this.style.sortTextColor
        )`${this.sortedColumnDirection === SortDirection.Ascending ? "↑" : "↓"}`;
      }
      header += columnTitle;
      header += " ".repeat(Math.max(0, this.columnWidths[i] - ansiLength(columnTitle)));
    }
    this.header.text = header;
    this.header.text.substring(0, width);
    this.header.style.color = this.style.headerTextColor;

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
      this.selectedIndex += this.clientHeight - 2;
      return;
    }

    if (isInputMatch(event, "pageup")) {
      this.selectedIndex -= this.clientHeight - 2;
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
