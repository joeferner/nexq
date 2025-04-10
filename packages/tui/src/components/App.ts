import { NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Header } from "./Header.js";
import { Queues } from "./Queues.js";

export class App extends Component {
    private readonly header: Header;
    private readonly queues: Queues;
    private readonly _children: Component[];

    public constructor(private readonly state: NexqState) {
        super();
        this.header = new Header(state);
        this.queues = new Queues(state);
        this._children = [new BoxComponent({
            direction: BoxDirection.Vertical,
            children: [this.header, this.queues]
        })];
    }

    public get children(): Component[] {
        return this._children;
    }
}
