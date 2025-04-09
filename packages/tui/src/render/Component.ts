import { Geometry } from "./Geometry.js";
import { RenderItem } from "./RenderItem.js";

export abstract class Component {
    public render(): RenderItem[] {
        const renderItems: RenderItem[] = [];
        for (const child of this.children) {
            renderItems.push(...child.render());
        }
        return renderItems;
    }

    public calculateGeometry(container: Geometry): void {
        for (const child of this.children) {
            child.calculateGeometry(container);
        }
    }

    public get geometry(): Geometry {
        if (this.children.length === 0) {
            return { left: 0, top: 0, height: 0, width: 0 };
        }
        if (this.children.length === 1) {
            return this.children[0].geometry;
        }
        throw new Error('if children is greater that 1 you must implement get geometry');
    }

    public abstract get children(): Component[];
}