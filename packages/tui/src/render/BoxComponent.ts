import { Component } from "./Component.js";
import { Geometry } from "./Geometry.js";
import { RenderItem } from "./RenderItem.js";

export enum BoxDirection {
    Vertical,
    Horizontal
}

export enum JustifyContent {
    Start,
    SpaceBetween,
    End
}

export interface BoxComponentOptions {
    direction: BoxDirection;
    justifyContent?: JustifyContent;
    maxHeight?: number;
    children: Component[];
}

export class BoxComponent extends Component {
    private _geometry: Geometry;
    private _children: Component[];
    public direction: BoxDirection;
    public justifyContent: JustifyContent | undefined;
    public maxHeight: number | undefined;

    public constructor(options: BoxComponentOptions) {
        super();
        this._children = options.children;
        this.direction = options.direction;
        this.justifyContent = options.justifyContent;
        this.maxHeight = options.maxHeight;
        this._geometry = { left: 0, top: 0, width: 0, height: 0 };
    }

    public get children(): Component[] {
        return this._children;
    }

    public override calculateGeometry(container: Geometry): void {
        const results: Geometry = {
            left: 0,
            top: 0,
            height: 0,
            width: 0
        };
        const remainingContainer: Geometry = structuredClone(container);
        if (this.maxHeight !== undefined) {
            remainingContainer.height = Math.min(remainingContainer.height, this.maxHeight);
        }
        for (const child of this.children) {
            child.calculateGeometry(remainingContainer);
            const childGeometry = child.geometry;
            if (this.direction === BoxDirection.Horizontal) {
                results.height = Math.max(results.height, childGeometry.height);
                results.width += childGeometry.width;
                remainingContainer.width -= childGeometry.width;
            } else {
                results.width = Math.max(results.width, childGeometry.width);
                results.height += childGeometry.height;
                remainingContainer.height -= childGeometry.height;
            }
        }
        if (this.justifyContent === JustifyContent.SpaceBetween || this.justifyContent === JustifyContent.End) {
            if (this.direction === BoxDirection.Horizontal) {
                results.width = container.width;
            } else {
                results.height = container.height;
            }
        }
        this._geometry = results;
    }

    public override get geometry(): Geometry {
        return this._geometry;
    }

    public render(): RenderItem[] {
        const renderItems: RenderItem[] = [];
        let x = this.geometry.left;
        let y = this.geometry.top;

        let gap = 0;
        if (this.justifyContent === JustifyContent.SpaceBetween) {
            if (this.direction === BoxDirection.Horizontal) {
                const totalChildrenWidth = this.children.reduce((p, v) => p + v.geometry.width, 0);
                gap = (this.geometry.width - totalChildrenWidth) / (this.children.length - 1);
            } else {
                const totalChildrenHeight = this.children.reduce((p, v) => p + v.geometry.height, 0);
                gap = (this.geometry.height - totalChildrenHeight) / (this.children.length - 1);
            }
        } else if (this.justifyContent === JustifyContent.End) {
            if (this.direction === BoxDirection.Horizontal) {
                const totalChildrenWidth = this.children.reduce((p, v) => p + v.geometry.width, 0);
                x += Math.max(0, this.geometry.width - totalChildrenWidth);
            } else {
                const totalChildrenHeight = this.children.reduce((p, v) => p + v.geometry.height, 0);
                y += Math.max(0, this.geometry.height - totalChildrenHeight);
            }
        }

        for (const child of this.children) {
            const childRenderItems = child.render();
            for (const childRenderItem of childRenderItems) {
                childRenderItem.geometry.left += x;
                childRenderItem.geometry.top += y;
                renderItems.push(childRenderItem);
            }

            if (this.direction === BoxDirection.Horizontal) {
                x += child.geometry.width;
                if (this.justifyContent === JustifyContent.SpaceBetween) {
                    x += gap;
                }
            } else {
                y += child.geometry.height;
                if (this.justifyContent === JustifyContent.SpaceBetween) {
                    y += gap;
                }
            }
        }
        return renderItems;
    }
}
