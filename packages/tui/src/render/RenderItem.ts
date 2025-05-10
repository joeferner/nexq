import { Geometry } from "./Geometry.js";
import { BorderStyle } from "./Style.js";

export interface TextRenderItem {
  type: "text";
  text: string;
  container: Geometry;
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

export interface BoxRenderItem {
  type: "box";
  container: Geometry;
  geometry: Geometry;
  zIndex: number;
  borderLeftStyle: BorderStyle;
  borderRightStyle: BorderStyle;
  borderTopStyle: BorderStyle;
  borderBottomStyle: BorderStyle;
  borderLeftColor: string;
  borderRightColor: string;
  borderTopColor: string;
  borderBottomColor: string;
}

export type RenderItem = TextRenderItem | CursorRenderItem | BoxRenderItem;
