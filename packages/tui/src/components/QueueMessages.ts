import { hex } from "ansis";
import matchPath from "node-match-path";
import * as R from "radash";
import { Align, FlexDirection } from "yoga-layout";
import { PeekMessagesResponseMessage } from "../client/NexqClientApi.js";
import { NexqStyles } from "../NexqStyles.js";
import { Box } from "../render/Box.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { BorderType } from "../render/RenderItem.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { App } from "./App.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";
import { SortDirection, TableView } from "./TableView.js";

const logger = createLogger("QueueMessages");

type QueueMessagesRow = PeekMessagesResponseMessage & { index: number };

export class QueueMessages extends Element {
  public static readonly PATH = "/queue/:queueName";
  private readonly tableView: TableView<QueueMessagesRow>;
  private readonly box: Box;
  private refreshTimeout?: NodeJS.Timeout;
  private inRefreshMessages = false;
  private queueName = "";
  public static readonly HELP_ITEMS: HelpItem[] = [
    {
      id: "delete",
      name: "Delete",
      shortcut: "ctrl-d",
    },
  ];

  public constructor(document: Document) {
    super(document);
    this.style.width = "100%";
    this.style.flexGrow = 1;
    this.style.alignItems = Align.Stretch;
    this.style.flexDirection = FlexDirection.Column;

    this.box = new Box(document);
    this.box.borderType = BorderType.Single;
    this.box.borderColor = NexqStyles.borderColor;
    this.box.style.flexGrow = 1;
    this.box.style.flexDirection = FlexDirection.Column;
    this.box.title = hex(NexqStyles.titleColor)` Messages `;
    this.box.style.alignItems = Align.Stretch;
    this.appendChild(this.box);

    this.tableView = new TableView(document, {
      ...NexqStyles.tableViewStyles,
      columns: [
        {
          title: "INDEX",
          align: "right",
          sortKeyboardShortcut: "shift-i",
          render: (row): string => `${row.index}`,
          sortItems: (rows, direction): QueueMessagesRow[] =>
            R.sort(rows, (row) => row.index, direction === SortDirection.Descending),
        },
        {
          title: "PRIORITY",
          align: "right",
          sortKeyboardShortcut: "shift-p",
          render: (row): string => `${row.priority}`,
          sortItems: (rows, direction): QueueMessagesRow[] =>
            R.sort(rows, (row) => row.priority, direction === SortDirection.Descending),
        },
        {
          title: "SENT",
          align: "left",
          sortKeyboardShortcut: "shift-s",
          render: (row): string => `${row.sentTime}`,
          sortItems: (rows, direction): QueueMessagesRow[] => R.alphabetical(rows, (row) => row.sentTime, direction),
        },
      ],
    });
    this.tableView.style.flexGrow = 1;
    this.box.appendChild(this.tableView);
  }

  protected override elementDidMount(): void {
    const m = matchPath.match(QueueMessages.PATH, this.window.location.pathname);
    if (!m.matches) {
      throw new Error(`invalid url: ${this.window.location.pathname}`);
    }
    const queueName = m.params?.["queueName"];
    if (!queueName) {
      throw new Error(`invalid url: ${this.window.location.pathname} (missing queue name)`);
    }

    this.queueName = queueName;
    this.box.title = hex(NexqStyles.titleColor)` "${this.queueName}" Messages `;

    const run = async (): Promise<void> => {
      await this.window.refresh();
      await this.refreshMessages();
      if (this.window.activeElement !== this.tableView) {
        this.tableView.focus();
      }
    };
    void run();
  }

  protected override elementWillUnmount(): void {
    if (this.refreshTimeout) {
      clearTimeout(this.refreshTimeout);
      this.refreshTimeout = undefined;
    }
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    for (const helpItem of QueueMessages.HELP_ITEMS) {
      if (isInputMatch(event, helpItem.shortcut)) {
        this.handleHelpShortcut(helpItem);
        return;
      }
    }

    if (isInputMatch(event, "return")) {
      const currentItem = this.tableView.getCurrentItem();
      if (currentItem) {
        this.window.history.pushState(
          {},
          "",
          `/queue/${encodeURIComponent(this.queueName)}/${encodeURIComponent(currentItem.id)}`
        );
      }
      return;
    }

    super.onKeyDown(event);
  }

  private async refreshMessages(): Promise<void> {
    const app = App.getApp(this);

    if (this.inRefreshMessages) {
      return;
    }
    try {
      this.inRefreshMessages = true;
      if (this.refreshTimeout) {
        clearTimeout(this.refreshTimeout);
        this.refreshTimeout = undefined;
      }
      logger.info("refreshMessages");
      const resp = await app.api.api.peekMessages(this.queueName, { maxNumberOfMessages: 100 });
      this.tableView.items = resp.data.messages.map((m, index) => ({ ...m, index }));
      await this.window.refresh();
    } catch (err) {
      StatusBar.setStatus(this.document, `Failed to peek messages`, err);
    } finally {
      this.refreshTimeout = setTimeout(() => {
        void this.refreshMessages();
      }, app.refreshInterval);
      this.inRefreshMessages = false;
    }
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "delete") {
      void this.deleteSelectedMessage();
    } else {
      logger.error(`unhandled help shortcut ${helpItem.id}`);
    }
  }

  private async deleteSelectedMessage(): Promise<void> {
    const messages = this.tableView.getSelectedItems();
    if (messages.length === 0) {
      StatusBar.setStatus(this.document, "No selected message to delete");
      return;
    }

    let messageIds = messages.map((r) => `"${r.id}"`).join(", ");
    if (messageIds.length > 40) {
      messageIds = messageIds.substring(0, 40) + "…";
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Delete Queue",
      message: `Are you sure you want to delete ${messageIds}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm === "Delete") {
      try {
        await Promise.all(messages.map((r) => app.api.api.deleteMessage(this.queueName, r.id)));
        StatusBar.setStatus(this.document, `${messageIds} deleted!`);
        void this.refreshMessages();
      } catch (err) {
        logger.error(`Failed to delete one or more messages`, err);
        StatusBar.setStatus(this.document, `Failed to delete one or more messages`, err);
      }
    }
  }
}
