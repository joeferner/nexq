import { NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection, JustifyContent } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { TextComponent } from "../render/TextComponent.js";

export class StatusBar extends Component {
    private _children: Component[];
    private textComponent = new TextComponent({ text: '' });

    public constructor(state: NexqState) {
        super();
        this._children = [new BoxComponent({
            children: [this.textComponent],
            direction: BoxDirection.Vertical,
            justifyContent: JustifyContent.Center,
            width: '100%'
        })]
    }

    public get children(): Component[] {
        return this._children;
    }
}