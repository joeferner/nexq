import { NexqState } from "../NexqState.js";
import { BoxBorder, BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { TextComponent } from "../render/TextComponent.js";

export class Queues extends Component {
    private readonly _children: Component[];

    public constructor(private readonly state: NexqState) {
        super();
        const t = new TextComponent({ text: ' ' });
        this._children = [new BoxComponent({
            children: [t],
            direction: BoxDirection.Vertical,
            border: BoxBorder.Single
        })];
        state.on('keypress', (chunk, key) => {
            t.text = key?.name ?? '?';
            state.emit('changed');
        });
    }

    public get children(): Component[] {
        return this._children;
    }
}