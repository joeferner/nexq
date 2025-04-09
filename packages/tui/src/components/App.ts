import { NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Header } from "./Header.js";

export class App extends Component {
    private readonly header: Header;
    private readonly _children: Component[];

    public constructor(private readonly state: NexqState) {
        super();
        this.header = new Header(state);
        this._children = [new BoxComponent({
            direction: BoxDirection.Vertical,
            children: [this.header]
        })];
    }

    public get children(): Component[] {
        return this._children;
    }
}
