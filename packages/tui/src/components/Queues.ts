import { GetQueueResponse } from "../client/NexqClientApi.js";
import { NexqState, Screen } from "../NexqState.js";
import { BoxBorder, BoxComponent, BoxDirection } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { Geometry } from "../render/Geometry.js";
import { TextComponent } from "../render/TextComponent.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { HelpItem } from "./Help.js";
import { SortDirection, TableView } from "./TableView.js";
import * as R from "radash";

const logger = createLogger("Queues");

export class Queues extends Component {
  public static readonly ID = "queues";
  private readonly _children: Component[];
  private readonly tableView: TableView<GetQueueResponse>;
  private refreshTimeout?: NodeJS.Timeout;
  public static readonly HELP_ITEMS: HelpItem[] = [
    {
      id: "purge",
      name: "Purge",
      shortcut: "ctrl-u",
    },
    {
      id: "delete",
      name: "Delete",
      shortcut: "ctrl-d",
    },
    {
      id: "pause",
      name: "Pause/Resume",
      shortcut: "ctrl-r",
    },
  ];

  public constructor(private readonly state: NexqState) {
    super();
    this.tableView = new TableView({
      columns: [
        {
          title: "NAME",
          align: "left",
          sortKeyboardShortcut: "shift-n",
          render: (queue): string => queue.name,
          sortItems: (rows, direction): GetQueueResponse[] => R.alphabetical(rows, (row) => row.name, direction),
        },
        {
          title: "COUNT",
          align: "right",
          sortKeyboardShortcut: "shift-c",
          render: (queue): string => `${queue.numberOfMessages}`,
          sortItems: (rows, direction): GetQueueResponse[] =>
            R.sort(rows, (row) => row.numberOfMessages, direction === SortDirection.Descending),
        },
        {
          title: "VISIBLE",
          align: "right",
          sortKeyboardShortcut: "shift-v",
          render: (queue): string => `${queue.numberOfMessagesVisible}`,
          sortItems: (rows, direction): GetQueueResponse[] =>
            R.sort(rows, (row) => row.numberOfMessagesVisible, direction === SortDirection.Descending),
        },
        {
          title: "NOT VISIBLE",
          align: "right",
          sortKeyboardShortcut: "shift-o",
          render: (queue): string => `${queue.numberOfMessagesNotVisible}`,
          sortItems: (rows, direction): GetQueueResponse[] =>
            R.sort(rows, (row) => row.numberOfMessagesNotVisible, direction === SortDirection.Descending),
        },
        {
          title: "DELAYED",
          align: "right",
          sortKeyboardShortcut: "shift-d",
          render: (queue): string => `${queue.numberOfMessagesDelayed}`,
          sortItems: (rows, direction): GetQueueResponse[] =>
            R.sort(rows, (row) => row.numberOfMessagesDelayed, direction === SortDirection.Descending),
        },
        {
          title: "STATUS",
          align: "left",
          sortKeyboardShortcut: "shift-s",
          render: (queue): string => `${queue.paused ? "P" : ""}`,
          sortItems: (rows, direction): GetQueueResponse[] =>
            R.sort(rows, (row) => (row.paused ? 1 : 0), direction === SortDirection.Descending),
        },
      ],
    });

    this._children = [
      new BoxComponent({
        children: [this.tableView],
        direction: BoxDirection.Vertical,
        border: BoxBorder.Single,
        borderColor: state.borderColor,
        width: "100%",
        height: "100%",
        title: new BoxComponent({
          direction: BoxDirection.Horizontal,
          children: [new TextComponent({ text: " Queues ", color: state.titleColor })],
        }),
      }),
    ];
    state.on("keypress", (_chunk, key) => {
      if (state.focus !== Queues.ID) {
        return;
      }
      let found = false;

      for (const helpItem of Queues.HELP_ITEMS) {
        if (isInputMatch(key, helpItem.shortcut)) {
          this.handleHelpShortcut(helpItem);
          found = true;
          break;
        }
      }

      if (this.tableView.handleKeyPress(key)) {
        found = true;
      }

      if (!found) {
        logger.debug("unhandled key press", JSON.stringify(key));
      }

      state.emit("changed");
    });
    state.on("screenChange", (newScreen: Screen) => {
      if (newScreen === Screen.Queues) {
        void this.refreshQueues();
      } else {
        if (this.refreshTimeout) {
          clearTimeout(this.refreshTimeout);
          this.refreshTimeout = undefined;
        }
      }
    });
  }

  private async refreshQueues(): Promise<void> {
    try {
      if (this.refreshTimeout) {
        clearTimeout(this.refreshTimeout);
        this.refreshTimeout = undefined;
      }
      const resp = await this.state.api.api.getQueues();
      this.tableView.items = resp.data.queues;
      this.state.emit("changed");
    } catch (err) {
      this.state.setStatus(`Failed to get queues`, err);
    } finally {
      this.refreshTimeout = setTimeout(() => {
        void this.refreshQueues();
      }, this.state.refreshInterval);
    }
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "purge") {
      void this.purgeSelectedQueues();
    } else if (helpItem.id === "delete") {
      void this.deleteSelectedQueues();
    } else if (helpItem.id === "pause") {
      void this.pauseResumeSelectedQueues();
    } else {
      throw new Error(`TODO ${helpItem.id}`);
    }
  }

  private async purgeSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      this.state.setStatus("No selected queues to purge");
      return;
    }

    let queueNames = queues.map((r) => `"${r.name}"`).join(", ");
    if (queueNames.length > 40) {
      queueNames = queueNames.substring(0, 40) + "…";
    }

    const confirm = await this.state.confirm({
      title: "Purge Queue",
      message: `Are you sure you want to purge ${queueNames}?`,
      options: ["Cancel", "Purge"],
      defaultOption: "Cancel",
    });
    if (confirm) {
      try {
        await Promise.all(queues.map((r) => this.state.api.api.purgeQueue(r.name)));
        this.state.setStatus(`${queueNames} purged`);
        void this.refreshQueues();
      } catch (err) {
        this.state.setStatus(`Failed to purge one or more queues`, err);
      }
    }
  }

  private async deleteSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      this.state.setStatus("No selected queues to delete");
      return;
    }

    let queueNames = queues.map((r) => `"${r.name}"`).join(", ");
    if (queueNames.length > 40) {
      queueNames = queueNames.substring(0, 40) + "…";
    }

    const confirm = await this.state.confirm({
      title: "Delete Queue",
      message: `Are you sure you want to delete ${queueNames}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm) {
      try {
        await Promise.all(queues.map((r) => this.state.api.api.deleteQueue(r.name)));
        this.state.setStatus(`${queueNames} deleted!`);
        void this.refreshQueues();
      } catch (err) {
        this.state.setStatus(`Failed to delete one or more queues`, err);
      }
    }
  }

  private async pauseResumeSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      this.state.setStatus("No selected queues to pause/resume");
      return;
    }

    try {
      let pausedCount = 0;
      let resumedCount = 0;
      await Promise.all(
        queues.map((q) => {
          if (q.paused) {
            resumedCount++;
            return this.state.api.api.resumeQueue(q.name);
          } else {
            pausedCount++;
            return this.state.api.api.pauseQueue(q.name);
          }
        })
      );
      if (queues.length === 1) {
        this.state.setStatus(`${queues[0].name} ${pausedCount > 0 ? "paused" : "resumed"}!`);
      } else {
        if (pausedCount > 0 && resumedCount > 0) {
          this.state.setStatus(`${pausedCount} queues paused and ${resumedCount} queues resumed!`);
        } else if (pausedCount > 0) {
          this.state.setStatus(`${pausedCount} queues paused!`);
        } else {
          this.state.setStatus(`${resumedCount} queues resumed!`);
        }
      }
      void this.refreshQueues();
    } catch (err) {
      this.state.setStatus(`Failed to delete one or more queues`, err);
    }
  }

  public get children(): Component[] {
    return this._children;
  }

  public override calculateGeometry(container: Geometry): void {
    container.height--;
    super.calculateGeometry(container);
  }
}
