import { Geometry } from "./Geometry.js";

export interface TextRenderItem {
  type: "text";
  text: string;
  geometry: Geometry;
  color: string;
  inverse?: boolean;
  zIndex: number;
}

export type RenderItem = TextRenderItem;
