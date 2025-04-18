import { NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Header } from "./Header.js";
import { Queues } from "./Queues.js";
import { StatusBar } from "./StatusBar.js";

export class App extends Component {
  private readonly header: Header;
  private readonly queues: Queues;
  private readonly statusBar: StatusBar;
  private readonly _children: Component[];

  public constructor(state: NexqState) {
    super();
    this.header = new Header(state);
    this.queues = new Queues(state);
    this.statusBar = new StatusBar(state);
    this._children = [
      new BoxComponent({
        direction: BoxDirection.Vertical,
        children: [state.confirmDialog, state.moveMessagesDialog, this.header, this.queues, this.statusBar],
      }),
    ];
  }

  public get children(): Component[] {
    return this._children;
  }
}
