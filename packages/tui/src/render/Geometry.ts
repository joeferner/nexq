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

export function rectIntersection(rect1: Geometry, rect2: Geometry): Geometry | undefined {
  const x1 = Math.max(rect1.left, rect2.left);
  const y1 = Math.max(rect1.top, rect2.top);
  const x2 = Math.min(rect1.left + rect1.width, rect2.left + rect2.width);
  const y2 = Math.min(rect1.top + rect1.height, rect2.top + rect2.height);

  const intersectionWidth = x2 - x1;
  const intersectionHeight = y2 - y1;

  if (intersectionWidth <= 0 || intersectionHeight <= 0) {
    return undefined;
  }

  return {
    left: x1,
    top: y1,
    width: intersectionWidth,
    height: intersectionHeight,
  };
}
