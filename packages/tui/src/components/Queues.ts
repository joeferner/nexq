import { Focus, NexqState } from "../NexqState.js";
import { BoxBorder, BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { logToFile } from "../utils/log.js";
import { TableView } from "./TableView.js";

interface QueueListViewItem {
  name: string;
  count: number;
}

export class Queues extends Component {
  private readonly _children: Component[];
  private readonly tableView: TableView<QueueListViewItem>;

  public constructor(private readonly state: NexqState) {
    super();
    this.tableView = new TableView({
      columns: [
        {
          title: 'NAME',
          align: 'left',
          render: (queue): string => queue.name
        },
        {
          title: 'COUNT',
          align: 'right',
          render: (queue): string => `${queue.count}`
        }
      ]
    });
    const items = [];
    for (let i = 0; i < 100; i++) {
      items.push({ name: `item ${i}`, count: i * 10 });
    }
    this.tableView.items = items;

    this._children = [
      new BoxComponent({
        children: [this.tableView],
        direction: BoxDirection.Vertical,
        border: BoxBorder.Single,
        width: "100%",
        height: "100%",
        title: "Queues",
      }),
    ];
    state.on("keypress", (_chunk, key) => {
      if (state.focus === Focus.Queues) {
        if (key?.name === 'down') {
          this.tableView.selectedIndex++;
        } else if (key?.name === 'up') {
          this.tableView.selectedIndex--;
        } else if (key?.name === 'pagedown') {
          this.tableView.selectedIndex += this.geometry.height - 3;
        } else if (key?.name === 'pageup') {
          this.tableView.selectedIndex -= this.geometry.height - 3;
        } else {
          logToFile(JSON.stringify(key));
        }
        state.emit("changed");
      }
    });
  }

  public get children(): Component[] {
    return this._children;
  }
}
