import * as R from "radash";
import { Align, FlexDirection, Overflow } from "yoga-layout";
import { GetTopicResponse } from "../client/NexqClientApi.js";
import { NexqStyles } from "../NexqStyles.js";
import { Box } from "../render/Box.js";
import { fgColor } from "../render/color.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { BorderType } from "../render/RenderItem.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { App } from "./App.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";
import { TableView } from "./TableView.js";

const logger = createLogger("Topics");

export class Topics extends Element {
  public static readonly PATH = "/topic";
  private readonly box: Box;
  private readonly tableView: TableView<GetTopicResponse>;
  private refreshTimeout?: NodeJS.Timeout;
  private inRefreshTopics = false;
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
    this.style.flexShrink = 1;
    this.style.alignItems = Align.Stretch;
    this.style.flexDirection = FlexDirection.Column;
    this.style.overflow = Overflow.Hidden;

    this.box = new Box(document);
    this.box.borderType = BorderType.Single;
    this.box.borderColor = NexqStyles.borderColor;
    this.box.style.flexGrow = 1;
    this.box.style.flexShrink = 1;
    this.box.style.flexDirection = FlexDirection.Column;
    this.box.style.alignItems = Align.Stretch;
    this.box.style.overflow = Overflow.Hidden;
    this.box.title = fgColor(NexqStyles.titleColor)` Topics `;
    this.appendChild(this.box);

    this.tableView = new TableView(document, {
      columns: [
        {
          title: "NAME",
          align: "left",
          sortKeyboardShortcut: "shift-n",
          render: (topic): string => topic.name,
          sortItems: (rows, direction): GetTopicResponse[] => R.alphabetical(rows, (row) => row.name, direction),
        },
      ],
    });
    this.tableView.style.flexGrow = 1;
    this.tableView.style.flexShrink = 1;
    NexqStyles.applyToTableView(this.tableView);
    this.box.appendChild(this.tableView);
  }

  protected override elementDidMount(): void {
    const run = async (): Promise<void> => {
      await this.window.refresh();
      await this.refreshTopics();
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
    for (const helpItem of Topics.HELP_ITEMS) {
      if (isInputMatch(event, helpItem.shortcut)) {
        this.handleHelpShortcut(helpItem);
        return;
      }
    }

    super.onKeyDown(event);
  }

  private async refreshTopics(): Promise<void> {
    const app = App.getApp(this);

    if (this.inRefreshTopics) {
      return;
    }
    try {
      this.inRefreshTopics = true;
      if (this.refreshTimeout) {
        clearTimeout(this.refreshTimeout);
        this.refreshTimeout = undefined;
      }
      logger.info("refreshTopics");
      const resp = await app.api.api.getTopics();
      const topics = resp.data.topics;
      this.box.title =
        fgColor(NexqStyles.titleColor)` Topics[` +
        fgColor(NexqStyles.titleCountColor)`${topics.length}` +
        fgColor(NexqStyles.titleColor)`] `;
      this.tableView.items = topics;
      await this.window.refresh();
    } catch (err) {
      StatusBar.setStatus(this.document, `Failed to get topics`, err);
    } finally {
      this.refreshTimeout = setTimeout(() => {
        void this.refreshTopics();
      }, app.refreshInterval);
      this.inRefreshTopics = false;
    }
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "delete") {
      void this.deleteSelectedTopics();
    } else {
      logger.error(`unhandled help shortcut ${helpItem.id}`);
    }
  }

  private async deleteSelectedTopics(): Promise<void> {
    const topics = this.tableView.getSelectedItems();
    if (topics.length === 0) {
      StatusBar.setStatus(this.document, "No selected topics to delete");
      return;
    }

    let topicNames = topics.map((r) => `"${r.name}"`).join(", ");
    if (topicNames.length > 40) {
      topicNames = topicNames.substring(0, 40) + "…";
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Delete Topic",
      message: `Are you sure you want to delete ${topicNames}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm === "Delete") {
      try {
        await Promise.all(topics.map((r) => app.api.api.deleteTopic(r.name)));
        StatusBar.setStatus(this.document, `${topicNames} deleted!`);
        void this.refreshTopics();
      } catch (err) {
        logger.error(`Failed to delete one or more topics`, err);
        StatusBar.setStatus(this.document, `Failed to delete one or more topics`, err);
      }
    }
  }
}
