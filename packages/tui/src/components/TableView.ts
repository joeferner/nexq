import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { TextComponent } from "../render/TextComponent.js";

export interface TableViewOptions<T> {
  columns: TableViewColumn<T>[];
}

export interface TableViewColumn<T> {
  title: string;
  align: 'left' | 'right';
  render: (value: T) => string;
}

export class TableView<T> extends Component {
  private readonly box = new BoxComponent({ children: [], direction: BoxDirection.Vertical });
  private readonly _children: Component[] = [this.box];
  private _items: T[] = [];
  public selectedIndex = 0;
  public offset = 0;
  public columns: TableViewColumn<T>[];
  private readonly columnWidths: number[] = [];

  public constructor(options: TableViewOptions<T>) {
    super();
    this.columns = options.columns;
  }

  public set items(items: T[]) {
    this._items = items;
    this.updateColumnWidths();
  }

  public get items(): T[] {
    return this._items;
  }

  public get children(): Component[] {
    return this._children;
  }

  public override calculateGeometry(container: Geometry): void {
    if (this._children.length !== container.height) {
      const children: TextComponent[] = [];
      for (let i = 0; i < container.height; i++) {
        children.push(new TextComponent({ text: 'aaaaaa', color: '#ffffff' }));
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
      children[i + 1].text = this.createRowText(this.items[index], container.width);
    }

    let header = '';
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      header += column.title;
      header += ' '.repeat(Math.max(0, this.columnWidths[i] - column.title.length));
    }
    children[0].text = header;

    super.calculateGeometry(container);
  }

  private createRowText(row: T, maxWidth: number): string {
    let text = '';
    for (let i = 0; i < this.columns.length; i++) {
      const column = this.columns[i];
      const columnText = column.render(row);
      const padding = ' '.repeat(this.columnWidths[i] - columnText.length - 1);
      if (column.align === 'left') {
        text += columnText + padding + ' ';
      } else {
        text += padding + columnText + ' ';
      }
    }
    text += ' '.repeat(Math.max(0, maxWidth - text.length));
    return text;
  }

  private updateColumnWidths(): void {
    this.columnWidths.length = 0;
    for (const item of this.items) {
      for (let i = 0; i < this.columns.length; i++) {
        const column = this.columns[i];
        const columnText = column.render(item);
        this.columnWidths[i] = Math.max(this.columnWidths[i] ?? 0, columnText.length + 1, column.title.length + 1);
      }
    }
  }
}
