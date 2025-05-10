import { Geometry } from "./Geometry.js";
import { BorderStyle } from "./Style.js";

export interface BaseRenderItem {
  name?: string;
  type: "text" | "cursor" | "box";
  geometry: Geometry;
  zIndex: number;
  canRenderOnBorder?: boolean;
}

export interface RenderItemWithChildren extends BaseRenderItem {
  children: RenderItem[];
}

export interface TextRenderItem extends BaseRenderItem {
  type: "text";
  text: string;
  color: string;
  bgColor?: string;
  inverse?: boolean;
}

export interface CursorRenderItem extends BaseRenderItem {
  type: "cursor";
}

export interface BoxRenderItem extends RenderItemWithChildren {
  type: "box";
  borderLeftStyle?: BorderStyle;
  borderRightStyle?: BorderStyle;
  borderTopStyle?: BorderStyle;
  borderBottomStyle?: BorderStyle;
  borderLeftColor?: string;
  borderRightColor?: string;
  borderTopColor?: string;
  borderBottomColor?: string;
}

export type RenderItem = TextRenderItem | CursorRenderItem | BoxRenderItem;

export function findRenderItem(start: RenderItem, fn: (item: RenderItem) => boolean): RenderItem | undefined {
  if (fn(start)) {
    return start;
  }
  if ("children" in start) {
    for (const child of start.children) {
      const foundItem = findRenderItem(child, fn);
      if (foundItem) {
        return foundItem;
      }
    }
  }
  return undefined;
}
