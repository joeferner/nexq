import { Component } from "./Renderer.js";
import { RenderItem } from "./RenderItem.js";

export interface TextComponentOptions {
    text: string;
    color?: string;
}

export class TextComponent extends Component {
    public constructor(private readonly options: TextComponentOptions) {
        super();
    }

    public getChildren(): Component[] {
        return [];
    }

    public override getRenderItems(): RenderItem[] {
        const lines = this.options.text.split('\n');
        const height = lines.length;
        const width = Math.max(...lines.map(l => l.length));
        return [{
            type: 'text',
            text: this.options.text,
            color: this.options.color ?? '#ffffff',
            zIndex: 0,
            geometry: {
                top: 0,
                left: 0,
                width,
                height
            }
        }];
    }
}
