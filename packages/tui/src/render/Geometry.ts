import { Node as YogaNode } from "yoga-layout/load";

export interface Geometry {
  left: number;
  top: number;
  width: number;
  height: number;
}

export function geometryFromYogaNode(node: YogaNode | undefined): Geometry {
  if (!node) {
    return { left: 0, top: 0, width: 0, height: 0 };
  }
  const layout = node.getComputedLayout();
  return {
    left: layout.left,
    top: layout.top,
    width: layout.width,
    height: layout.height,
  };
}
