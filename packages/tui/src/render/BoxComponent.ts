import * as R from "radash";
import { Component } from "./Component.js";
import { Geometry } from "./Geometry.js";
import { RenderItem } from "./RenderItem.js";

export enum BoxDirection {
  Vertical = "Vertical",
  Horizontal = "Horizontal",
}

export enum BoxBorder {
  Single = "Single",
}

export enum JustifyContent {
  Start = "Start",
  SpaceBetween = "SpaceBetween",
  Center = "Center",
  End = "End",
}

export interface BoxComponentOptions {
  direction: BoxDirection;
  justifyContent?: JustifyContent;
  height?: number | string;
  width?: number | string;
  children: Component[];
  border?: BoxBorder;
  borderColor?: string;
  background?: boolean;
  zIndex?: number;
  title?: Component;
}

interface BorderCharacters {
  nw: string;
  n: string;
  ne: string;
  e: string;
  se: string;
  s: string;
  sw: string;
  w: string;
}

const BORDERS: Record<BoxBorder, BorderCharacters> = {
  [BoxBorder.Single]: {
    nw: "┌",
    n: "─",
    ne: "┐",
    e: "│",
    se: "┘",
    s: "─",
    sw: "└",
    w: "│",
  },
};

export class BoxComponent extends Component {
  private _geometry: Geometry;
  public children: Component[];
  public direction: BoxDirection;
  public justifyContent: JustifyContent | undefined;
  public height: number | string | undefined;
  public width: number | string | undefined;
  public border: BoxBorder | undefined;
  public borderColor: string;
  public background: boolean;
  public _zIndex: number;
  public title: Component | undefined;

  public constructor(options: BoxComponentOptions) {
    super();
    this.children = options.children;
    this.direction = options.direction;
    this.justifyContent = options.justifyContent;
    this.height = options.height;
    this.width = options.width;
    this.border = options.border;
    this.borderColor = options.borderColor ?? "#ffffff";
    this.title = options.title;
    this.background = options.background ?? false;
    this._zIndex = options.zIndex ?? 0;
    this._geometry = { left: 0, top: 0, width: 0, height: 0 };
  }

  public get zIndex(): number {
    return this._zIndex;
  }

  public override calculateGeometry(container: Geometry): void {
    const results: Geometry = {
      left: 0,
      top: 0,
      height: 0,
      width: 0,
    };
    const remainingContainer: Geometry = structuredClone(container);
    if (this.height !== undefined) {
      remainingContainer.height = fromNumberOrPercent(this.height, remainingContainer.height);
    }
    if (this.width !== undefined) {
      remainingContainer.width = fromNumberOrPercent(this.width, remainingContainer.width);
    }
    if (this.border) {
      remainingContainer.top += 1;
      remainingContainer.left += 1;
      remainingContainer.width -= 2;
      remainingContainer.height -= 2;
    }
    for (const child of this.children) {
      child.calculateGeometry(remainingContainer);
      const childGeometry = child.geometry;
      if (this.direction === BoxDirection.Horizontal) {
        results.height = Math.max(results.height, childGeometry.height);
        results.width += childGeometry.width;
        remainingContainer.width -= childGeometry.width;
      } else {
        results.width = Math.max(results.width, childGeometry.width);
        results.height += childGeometry.height;
        remainingContainer.height -= childGeometry.height;
      }
    }
    if (this.justifyContent === JustifyContent.SpaceBetween || this.justifyContent === JustifyContent.End) {
      if (this.direction === BoxDirection.Horizontal) {
        results.width = container.width;
      } else {
        results.height = container.height;
      }
    }
    if (this.border) {
      results.width += 2;
      results.height += 2;
    }
    if (this.height !== undefined) {
      results.height = fromNumberOrPercent(this.height, container.height);
    }
    if (this.width !== undefined) {
      results.width = fromNumberOrPercent(this.width, container.width);
    }
    this._geometry = results;
  }

  public override get geometry(): Geometry {
    return this._geometry;
  }

  public render(): RenderItem[] {
    const renderItems: RenderItem[] = [];
    let x = this.geometry.left;
    let y = this.geometry.top;

    if (this.border) {
      this.renderBorder(renderItems);
      x++;
      y++;
    }

    let gap = 0;
    if (this.justifyContent === JustifyContent.SpaceBetween) {
      if (this.direction === BoxDirection.Horizontal) {
        const totalChildrenWidth = this.children.reduce((p, v) => p + v.geometry.width, 0);
        gap = Math.floor((this.geometry.width - totalChildrenWidth) / (this.children.length - 1));
      } else {
        const totalChildrenHeight = this.children.reduce((p, v) => p + v.geometry.height, 0);
        gap = Math.floor((this.geometry.height - totalChildrenHeight) / (this.children.length - 1));
      }
    } else if (this.justifyContent === JustifyContent.End) {
      if (this.direction === BoxDirection.Horizontal) {
        const totalChildrenWidth = this.children.reduce((p, v) => p + v.geometry.width, 0);
        x += Math.max(0, this.geometry.width - totalChildrenWidth);
      } else {
        const totalChildrenHeight = this.children.reduce((p, v) => p + v.geometry.height, 0);
        y += Math.max(0, this.geometry.height - totalChildrenHeight);
      }
    }

    for (const child of this.children) {
      const childRenderItems = child.render();
      for (const childRenderItem of childRenderItems) {
        childRenderItem.geometry.left += x;
        childRenderItem.geometry.top += y;
        if (this.justifyContent === JustifyContent.Center) {
          if (this.direction === BoxDirection.Horizontal) {
            throw new Error("center/horizontal not supported");
          } else {
            childRenderItem.geometry.left += Math.floor((this.geometry.width - child.geometry.width) / 2);
          }
        }
        renderItems.push(childRenderItem);
      }

      if (this.direction === BoxDirection.Horizontal) {
        x += child.geometry.width;
        if (this.justifyContent === JustifyContent.SpaceBetween) {
          x += gap;
        }
      } else {
        y += child.geometry.height;
        if (this.justifyContent === JustifyContent.SpaceBetween) {
          y += gap;
        }
      }
    }

    if (this.background) {
      let backgroundText = "";
      for (let y = 0; y < this.geometry.height; y++) {
        backgroundText += " ".repeat(this.geometry.width);
        backgroundText += "\n";
      }
      renderItems.push({
        type: "text",
        zIndex: -0.0001,
        color: "#000000",
        geometry: { ...this.geometry },
        text: backgroundText,
      });
    }

    for (const renderItem of renderItems) {
      renderItem.zIndex += this.zIndex;
    }
    return renderItems;
  }

  private renderBorder(renderItems: RenderItem[]): void {
    if (!this.border) {
      return;
    }

    const border = BORDERS[this.border];
    const geometry = this.geometry;

    // east/west
    for (let y = 1; y < geometry.height - 1; y++) {
      // west
      renderItems.push({
        type: "text",
        text: border.w,
        color: this.borderColor,
        zIndex: this._zIndex,
        geometry: {
          top: geometry.top + y,
          left: geometry.left,
          width: 1,
          height: 1,
        },
      });

      // east
      renderItems.push({
        type: "text",
        text: border.e,
        color: this.borderColor,
        zIndex: this._zIndex,
        geometry: {
          top: geometry.top + y,
          left: geometry.left + geometry.width - 1,
          width: 1,
          height: 1,
        },
      });
    }

    // north/south
    for (let x = 1; x < geometry.width - 1; x++) {
      // north
      renderItems.push({
        type: "text",
        text: border.n,
        color: this.borderColor,
        zIndex: this._zIndex,
        geometry: {
          top: geometry.top,
          left: geometry.left + x,
          width: 1,
          height: 1,
        },
      });

      // south
      renderItems.push({
        type: "text",
        text: border.s,
        color: this.borderColor,
        zIndex: this._zIndex,
        geometry: {
          top: geometry.top + geometry.height - 1,
          left: geometry.left + x,
          width: 1,
          height: 1,
        },
      });
    }

    // north west corner
    renderItems.push({
      type: "text",
      text: border.nw,
      color: this.borderColor,
      zIndex: this._zIndex,
      geometry: {
        top: geometry.top,
        left: geometry.left,
        width: 1,
        height: 1,
      },
    });

    // north east corner
    renderItems.push({
      type: "text",
      text: border.ne,
      color: this.borderColor,
      zIndex: this._zIndex,
      geometry: {
        top: geometry.top,
        left: geometry.left + geometry.width - 1,
        width: 1,
        height: 1,
      },
    });

    // south west corner
    renderItems.push({
      type: "text",
      text: border.sw,
      color: this.borderColor,
      zIndex: this._zIndex,
      geometry: {
        top: geometry.top + geometry.height - 1,
        left: geometry.left,
        width: 1,
        height: 1,
      },
    });

    // south east corner
    renderItems.push({
      type: "text",
      text: border.se,
      color: this.borderColor,
      zIndex: this._zIndex,
      geometry: {
        top: geometry.top + geometry.height - 1,
        left: geometry.left + geometry.width - 1,
        width: 1,
        height: 1,
      },
    });

    // title
    if (this.title) {
      const titleContainer: Geometry = {
        left: 0,
        top: 0,
        width: geometry.width - 2,
        height: 1,
      };
      this.title.calculateGeometry(titleContainer);
      this.title.geometry.top = geometry.top;
      this.title.geometry.left = geometry.left + Math.floor((geometry.width - (this.title.geometry.width + 2)) / 2);
      const titleItems = this.title.render();
      for (const titleItem of titleItems) {
        titleItem.zIndex = this._zIndex + 0.0001;
        renderItems.push(titleItem);
      }
    }
  }
}

function fromNumberOrPercent(v: number | string, containerWidth: number): number {
  if (R.isNumber(v)) {
    return v;
  } else if (R.isString(v)) {
    if (v.endsWith("%")) {
      const percent = parseFloat(v) / 100;
      return Math.ceil(containerWidth * percent);
    } else {
      throw new Error(`invalid width "${v}"`);
    }
  } else {
    throw new Error(`invalid width "${v}"`);
  }
}
