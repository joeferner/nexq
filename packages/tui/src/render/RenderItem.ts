import { Geometry } from "./Geometry.js";

export interface TextRenderItem {
  type: "text";
  text: string;
  geometry: Geometry;
  color: string;
  zIndex: number;
}

export type RenderItem = TextRenderItem;
