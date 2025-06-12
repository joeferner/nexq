import matchPath from "node-match-path";
import { Align, FlexDirection, Overflow } from "yoga-layout";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { NexqStyles } from "../NexqStyles.js";
import { fgColor } from "../render/color.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { Text } from "../render/Text.js";
import { isInputMatch } from "../utils/input.js";
import { App } from "./App.js";
import { attributesToString, Detail, detailsToString, formatBytes, formatMs } from "./details.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";
import { logger } from "@nexq/logger";

const log = logger.getLogger("DescribeQueue");

export class DescribeQueue extends DivElement {
  public static readonly PATH = "/queue/:queueName/describe";
  private readonly text: Text;
  public static readonly HELP_ITEMS: HelpItem[] = [
    {
      id: "delete",
      name: "Delete",
      shortcut: "ctrl-d",
    },
  ];
  private queueName: string | undefined;

  public constructor(document: Document) {
    super(document);

    this.style.width = "100%";
    this.style.flexGrow = 1;
    this.style.flexShrink = 1;
    this.style.alignItems = Align.Stretch;
    this.style.flexDirection = FlexDirection.Column;
    this.style.overflowY = Overflow.Scroll;
    this.style.borderStyle = "solid";
    this.style.borderColor = NexqStyles.borderColor;
    this.borderTitle = fgColor(NexqStyles.titleColor)` Info `;

    this.text = new Text(document, { text: "" });
    this.text.style.flexGrow = 1;
    this.appendChild(this.text);
  }

  protected override elementDidMount(): void {
    const m = matchPath.match(DescribeQueue.PATH, this.window.location.pathname);
    if (!m.matches) {
      throw new Error(`invalid url: ${this.window.location.pathname}`);
    }
    this.queueName = m.params?.["queueName"];
    if (!this.queueName) {
      throw new Error(`invalid url: ${this.window.location.pathname} (missing queueName)`);
    }

    const queue = this.window.history.state as GetQueueResponse;
    const details: Detail[] = [
      { title: "Name", value: queue.name },
      { title: "Created At", value: queue.created ?? "<not set>" },
      { title: "Last Modified At", value: queue.lastModified ?? "<not set>" },
      { title: "Expires At", value: queue.expiresAt ?? "<not set>" },
      { title: "Expires", value: formatMs(queue.expiresMs) ?? "<not set>" },
      { title: "Delay", value: formatMs(queue.delayMs) ?? "<not set>" },
      { title: "Max Message Size", value: formatBytes(queue.maxMessageSize) ?? "<not set>" },
      { title: "Msg Retention Period", value: formatMs(queue.messageRetentionPeriodMs) ?? "<not set>" },
      { title: "Recv Message Wait Time", value: formatMs(queue.receiveMessageWaitTimeMs) ?? "<not set>" },
      { title: "Visibility Timeout", value: formatMs(queue.visibilityTimeoutMs) ?? "<not set>" },
      { title: "Max Receive Count", value: queue.maxReceiveCount?.toLocaleString() ?? "<not set>" },
      { title: "Paused", value: queue.paused ? "yes" : "no" },
      { title: "Dead Letter Queue Name", value: queue.deadLetterQueueName?.toLocaleString() ?? "<not set>" },
      { title: "Dead Letter Topic Name", value: queue.deadLetterTopicName?.toLocaleString() ?? "<not set>" },
      { title: "Tags", value: attributesToString(queue.tags) },
    ];
    this.text.text = detailsToString(details);

    this.borderTitle =
      fgColor(NexqStyles.titleColor)` Queue Info(` +
      fgColor(NexqStyles.titleAltColor)`${this.queueName}` +
      fgColor(NexqStyles.titleColor)`) `;

    const run = async (): Promise<void> => {
      await this.window.refresh();
      if (this.window.activeElement !== this) {
        this.focus();
      }
    };
    void run();
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    for (const helpItem of DescribeQueue.HELP_ITEMS) {
      if (isInputMatch(event, helpItem.shortcut)) {
        this.handleHelpShortcut(helpItem);
        return;
      }
    }

    super.onKeyDown(event);
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "delete") {
      void this.deleteQueue();
    } else {
      log.error(`unhandled help shortcut ${helpItem.id}`);
    }
  }

  private async deleteQueue(): Promise<void> {
    if (!this.queueName) {
      throw new Error("invalid state, missing queueName");
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Delete Queue",
      message: `Are you sure you want to delete ${this.queueName}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm === "Delete") {
      try {
        await app.api.api.deleteQueue(this.queueName);
        StatusBar.setStatus(this.document, `${this.queueName} deleted!`);
        this.window.history.popState();
      } catch (err) {
        log.error(`Failed to delete queue`, err);
        StatusBar.setStatus(this.document, `Failed to delete queue`, err);
      }
    }
  }
}
