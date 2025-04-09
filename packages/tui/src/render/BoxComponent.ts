import { logToFile } from "../utils/log.js";
import { Geometry } from "./Geometry.js";
import { Component } from "./Renderer.js";
import { RenderItem } from "./RenderItem.js";

export enum BoxDirection {
    Vertical,
    Horizontal
}

export interface BoxComponentOptions {
    direction: BoxDirection;
    children: Component[];
}

export class BoxComponent extends Component {
    private readonly options: BoxComponentOptions;

    public constructor(options: BoxComponentOptions) {
        super();
        this.options = structuredClone(options);
        this.options.children = options.children;
    }

    public get children(): Component[] {
        return this.options.children;
    }

    public override get geometry(): Geometry {
        const results: Geometry = {
            left: 0,
            top: 0,
            height: 0,
            width: 0
        };
        for (const child of this.children) {
            const childGeometry = child.geometry;
            if (this.options.direction === BoxDirection.Horizontal) {
                results.height = Math.max(results.height, childGeometry.height);
                results.width += childGeometry.width;
            } else {
                results.width = Math.max(results.width, childGeometry.width);
                results.height += childGeometry.height;
            }
        }
        return results;
    }

    public render(container: Geometry): RenderItem[] {
        const myGeometry = this.geometry;
        const renderItems: RenderItem[] = [];
        let x = myGeometry.left;
        let y = myGeometry.top;
        for (const child of this.children) {
            const childRenderItems = child.render(container);
            for (const childRenderItem of childRenderItems) {
                childRenderItem.geometry.left += x;
                childRenderItem.geometry.top += y;
                renderItems.push(childRenderItem);
            }

            if (this.options.direction === BoxDirection.Horizontal) {
                x += child.geometry.width;
            } else {
                y += child.geometry.height;
            }
        }
        return renderItems;
    }
}
