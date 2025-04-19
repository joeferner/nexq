import { Align, FlexDirection } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { Header } from "./Header.js";
import { Queues } from "./Queues.js";
import { StatusBar } from "./StatusBar.js";

export class App extends Component {
  private readonly header: Header;
  private readonly queues: Queues;
  private readonly statusBar: StatusBar;

  public constructor(state: NexqState) {
    super();
    this.header = new Header(state);
    this.queues = new Queues(state);
    this.statusBar = new StatusBar(state);
    this.width = "100%";
    this.height = "100%";
    this.flexDirection = FlexDirection.Column;
    this.alignItems = Align.Stretch;
    this.children.push(state.confirmDialog);
    this.children.push(state.moveMessagesDialog);
    this.children.push(this.header);
    this.children.push(this.queues);
    this.children.push(this.statusBar);
  }
}
