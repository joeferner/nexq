import * as R from "radash";
import { Align, FlexDirection, Overflow } from "yoga-layout";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { NexqStyles } from "../NexqStyles.js";
import { fgColor } from "../render/color.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { App, FilterEvent } from "./App.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";
import { SortDirection, TableView } from "./TableView.js";

const logger = createLogger("Queues");

export class Queues extends DivElement {
  public static readonly PATH = "/queue";
  private readonly tableView: TableView<GetQueueResponse>;
  private refreshTimeout?: NodeJS.Timeout;
  private inRefreshQueues = false;
  private readonly handleFilter: (event: FilterEvent) => unknown;
  private queues: GetQueueResponse[] = [];
  private filter?: RegExp;
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
    {
      id: "move",
      name: "Move Messages",
      shortcut: "ctrl-o",
    },
    {
      id: "describe",
      name: "Describe",
      shortcut: "d",
    },
  ];

  public constructor(document: Document) {
    super(document);
    this.id = "Queues";

    this.handleFilter = this._handleFilter.bind(this);

    this.style.width = "100%";
    this.style.flexGrow = 1;
    this.style.flexShrink = 1;
    this.style.alignItems = Align.Stretch;
    this.style.flexDirection = FlexDirection.Column;
    this.style.overflow = Overflow.Hidden;
    this.style.borderStyle = "solid";
    this.style.borderColor = NexqStyles.borderColor;
    this.borderTitle = fgColor(NexqStyles.titleColor)` Queues `;

    this.tableView = new TableView(document, {
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
    this.tableView.style.flexGrow = 1;
    this.tableView.style.flexShrink = 1;
    NexqStyles.applyToTableView(this.tableView);
    this.appendChild(this.tableView);
  }

  protected override elementDidMount(): void {
    App.getApp(this).addEventListener("filter", this.handleFilter);
    const run = async (): Promise<void> => {
      await this.window.refresh();
      await this.refreshQueues();
      if (this.window.activeElement !== this.tableView) {
        this.tableView.focus();
      }
    };
    void run();
  }

  protected override elementWillUnmount(): void {
    App.getApp(this).removeEventListener("filter", this.handleFilter);
    if (this.refreshTimeout) {
      clearTimeout(this.refreshTimeout);
      this.refreshTimeout = undefined;
    }
  }

  protected _handleFilter(event: FilterEvent): void {
    this.filter = event.value;
    this.tableView.items = this.filterQueues(this.queues, this.filter);
    void this.window.refresh();
  }

  private filterQueues(queues: GetQueueResponse[], filter: RegExp | undefined): GetQueueResponse[] {
    if (!filter) {
      return queues;
    }
    return queues.filter((q) => q.name.match(filter));
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    for (const helpItem of Queues.HELP_ITEMS) {
      if (isInputMatch(event, helpItem.shortcut)) {
        this.handleHelpShortcut(helpItem);
        return;
      }
    }

    if (isInputMatch(event, "return")) {
      const currentItem = this.tableView.getCurrentItem();
      if (currentItem) {
        this.window.history.pushState({}, "", `/queue/${encodeURIComponent(currentItem.name)}`);
      }
      return;
    }

    super.onKeyDown(event);
  }

  private async refreshQueues(): Promise<void> {
    const app = App.getApp(this);

    if (this.inRefreshQueues) {
      return;
    }
    try {
      this.inRefreshQueues = true;
      if (this.refreshTimeout) {
        clearTimeout(this.refreshTimeout);
        this.refreshTimeout = undefined;
      }
      logger.info("refreshQueues");
      const resp = await app.api.api.getQueues();
      this.queues = resp.data.queues;
      this.borderTitle =
        fgColor(NexqStyles.titleColor)` Queues[` +
        fgColor(NexqStyles.titleCountColor)`${this.queues.length}` +
        fgColor(NexqStyles.titleColor)`] `;
      this.tableView.items = this.filterQueues(this.queues, this.filter);
      await this.window.refresh();
    } catch (err) {
      StatusBar.setStatus(this.document, `Failed to get queues`, err);
    } finally {
      this.refreshTimeout = setTimeout(() => {
        void this.refreshQueues();
      }, app.refreshInterval);
      this.inRefreshQueues = false;
    }
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "purge") {
      void this.purgeSelectedQueues();
    } else if (helpItem.id === "delete") {
      void this.deleteSelectedQueues();
    } else if (helpItem.id === "pause") {
      void this.pauseResumeSelectedQueues();
    } else if (helpItem.id === "move") {
      void this.moveMessagesFromSelectedQueue();
    } else if (helpItem.id === "describe") {
      this.describeSelectedQueue();
    } else {
      logger.error(`unhandled help shortcut ${helpItem.id}`);
    }
  }

  private describeSelectedQueue(): void {
    const currentItem = this.tableView.getCurrentItem();
    if (currentItem) {
      this.window.history.pushState(currentItem, "", `/queue/${encodeURIComponent(currentItem.name)}/describe`);
    }
  }

  private async moveMessagesFromSelectedQueue(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      StatusBar.setStatus(this.document, "No selected queues to move messages");
      return;
    }
    if (queues.length > 1) {
      StatusBar.setStatus(this.document, "Can only move messages from one queue at a time");
      return;
    }

    const app = App.getApp(this);

    const sourceQueueName = queues[0].name;
    const result = await app.moveMessagesDialog.show({
      sourceQueueName,
    });
    if (result) {
      try {
        await app.api.api.moveMessages(sourceQueueName, { targetQueueName: result.targetQueueName });
        StatusBar.setStatus(this.document, `Messages moved from "${sourceQueueName}" to "${result.targetQueueName}"`);
        void this.refreshQueues();
      } catch (err) {
        logger.error(`failed to move messages from "${sourceQueueName}" to "${result.targetQueueName}"`, err);
        StatusBar.setStatus(this.document, `Failed to move messages`, err);
      }
    }
  }

  private async purgeSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      StatusBar.setStatus(this.document, "No selected queues to purge");
      return;
    }

    let queueNames = queues.map((r) => `"${r.name}"`).join(", ");
    if (queueNames.length > 40) {
      queueNames = queueNames.substring(0, 40) + "…";
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Purge Queue",
      message: `Are you sure you want to purge ${queueNames}?`,
      options: ["Cancel", "Purge"],
      defaultOption: "Cancel",
    });
    if (confirm === "Purge") {
      try {
        await Promise.all(queues.map((r) => app.api.api.purgeQueue(r.name)));
        StatusBar.setStatus(this.document, `${queueNames} purged`);
        void this.refreshQueues();
      } catch (err) {
        logger.error(`Failed to purge one or more queues`, err);
        StatusBar.setStatus(this.document, `Failed to purge one or more queues`, err);
      }
    }
  }

  private async deleteSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      StatusBar.setStatus(this.document, "No selected queues to delete");
      return;
    }

    let queueNames = queues.map((r) => `"${r.name}"`).join(", ");
    if (queueNames.length > 40) {
      queueNames = queueNames.substring(0, 40) + "…";
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Delete Queue",
      message: `Are you sure you want to delete ${queueNames}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm === "Delete") {
      try {
        await Promise.all(queues.map((r) => app.api.api.deleteQueue(r.name)));
        StatusBar.setStatus(this.document, `${queueNames} deleted!`);
        void this.refreshQueues();
      } catch (err) {
        logger.error(`Failed to delete one or more queues`, err);
        StatusBar.setStatus(this.document, `Failed to delete one or more queues`, err);
      }
    }
  }

  private async pauseResumeSelectedQueues(): Promise<void> {
    const queues = this.tableView.getSelectedItems();
    if (queues.length === 0) {
      StatusBar.setStatus(this.document, "No selected queues to pause/resume");
      return;
    }

    const app = App.getApp(this);

    try {
      let pausedCount = 0;
      let resumedCount = 0;
      await Promise.all(
        queues.map((q) => {
          if (q.paused) {
            resumedCount++;
            return app.api.api.resumeQueue(q.name);
          } else {
            pausedCount++;
            return app.api.api.pauseQueue(q.name);
          }
        })
      );
      if (queues.length === 1) {
        StatusBar.setStatus(this.document, `${queues[0].name} ${pausedCount > 0 ? "paused" : "resumed"}!`);
      } else {
        if (pausedCount > 0 && resumedCount > 0) {
          StatusBar.setStatus(this.document, `${pausedCount} queues paused and ${resumedCount} queues resumed!`);
        } else if (pausedCount > 0) {
          StatusBar.setStatus(this.document, `${pausedCount} queues paused!`);
        } else {
          StatusBar.setStatus(this.document, `${resumedCount} queues resumed!`);
        }
      }
      void this.refreshQueues();
    } catch (err) {
      logger.error(`Failed to pause/resume one or more queues`, err);
      StatusBar.setStatus(this.document, `Failed to pause/resume one or more queues`, err);
    }
  }
}
