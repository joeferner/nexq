import { Geometry } from "./Geometry.js";

export interface TextRenderItem {
  type: "text";
  text: string;
  geometry: Geometry;
  color: string;
  bgColor?: string;
  inverse?: boolean;
  zIndex: number;
}

export interface CursorRenderItem {
  type: "cursor";
  geometry: Geometry;
  zIndex: number;
}

export type RenderItem = TextRenderItem | CursorRenderItem;
