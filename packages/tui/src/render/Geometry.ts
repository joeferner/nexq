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
  return {
    left: node.getComputedLeft(),
    top: node.getComputedTop(),
    width: node.getComputedWidth(),
    height: node.getComputedHeight(),
  };
}
