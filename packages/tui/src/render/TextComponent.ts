import { Component } from "./Component.js";
import { Geometry } from "./Geometry.js";
import { RenderItem } from "./RenderItem.js";

export interface TextComponentOptions {
    text: string;
    color?: string;
}

export class TextComponent extends Component {
    public text: string;
    public color: string | undefined;
    private _geometry: Geometry;

    public constructor(options: TextComponentOptions) {
        super();
        this.text = options.text;
        this.color = options.color;
        this._geometry = { left: 0, top: 0, width: 0, height: 0 };
    }

    public get children(): Component[] {
        return [];
    }

    public override get geometry(): Geometry {
        return this._geometry;
    }

    public calculateGeometry(_container: Geometry): void {
        // TODO wrap text
        const lines = this.text.split('\n');
        const height = lines.length;
        const width = Math.max(...lines.map(l => l.length));
        this._geometry = {
            top: 0,
            left: 0,
            width,
            height
        };
    }

    public override render(): RenderItem[] {
        return [{
            type: 'text',
            text: this.text,
            color: this.color ?? '#ffffff',
            zIndex: 0,
            geometry: this.geometry
        }];
    }
}
