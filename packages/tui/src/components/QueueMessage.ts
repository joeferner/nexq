import matchPath from "node-match-path";
import { Align, FlexDirection, Overflow } from "yoga-layout";
import { PeekMessagesResponseMessage } from "../client/NexqClientApi.js";
import { NexqStyles } from "../NexqStyles.js";
import { fgColor } from "../render/color.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { Text } from "../render/Text.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { App } from "./App.js";
import { Detail, detailsToString } from "./details.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";

const logger = createLogger("QueueMessage");

export class QueueMessage extends DivElement {
  public static readonly PATH = "/queue/:queueName/:messageId";
  private readonly text: Text;
  public static readonly HELP_ITEMS: HelpItem[] = [
    {
      id: "delete",
      name: "Delete",
      shortcut: "ctrl-d",
    },
  ];
  private queueName: string | undefined;
  private messageId: string | undefined;

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
    this.borderTitle = fgColor(NexqStyles.titleColor)` Messages `;

    this.text = new Text(document, { text: "" });
    this.text.style.flexGrow = 1;
    this.appendChild(this.text);
  }

  protected override elementDidMount(): void {
    const m = matchPath.match(QueueMessage.PATH, this.window.location.pathname);
    if (!m.matches) {
      throw new Error(`invalid url: ${this.window.location.pathname}`);
    }
    this.queueName = m.params?.["queueName"];
    if (!this.queueName) {
      throw new Error(`invalid url: ${this.window.location.pathname} (missing queueName)`);
    }
    this.messageId = m.params?.["messageId"];
    if (!this.messageId) {
      throw new Error(`invalid url: ${this.window.location.pathname} (missing messageId)`);
    }

    const message = this.window.history.state as PeekMessagesResponseMessage;
    const details: Detail[] = [
      { title: "Id", value: message.id },
      { title: "Priority", value: `${message.priority}` },
      { title: "Available", value: message.isAvailable ? "Yes" : "No" },
      { title: "Receive Count", value: message.receiveCount.toLocaleString() },
      { title: "Sent Time", value: message.sentTime },
      { title: "Delay Until", value: message.delayUntil ?? "<not set>" },
      { title: "Expires At", value: message.expiresAt ?? "<not set>" },
      { title: "First Received At", value: message.expiresAt ?? "<not set>" },
      { title: "Receipt Handle", value: message.receiptHandle ?? "<not set>" },
      { title: "Last Nak", value: message.lastNakReason ?? "<not set>" },
      { title: "Attributes", value: attributesToString(message.attributes) },
      { title: "Body", value: bodyToString(message.body), newLineNoIndent: true },
    ];
    this.text.text = detailsToString(details);

    this.borderTitle =
      fgColor(NexqStyles.titleColor)` Message(` +
      fgColor(NexqStyles.titleAltColor)`${this.queueName}/${this.messageId}` +
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
    for (const helpItem of QueueMessage.HELP_ITEMS) {
      if (isInputMatch(event, helpItem.shortcut)) {
        this.handleHelpShortcut(helpItem);
        return;
      }
    }

    super.onKeyDown(event);
  }

  private handleHelpShortcut(helpItem: HelpItem): void {
    if (helpItem.id === "delete") {
      void this.deleteMessage();
    } else {
      logger.error(`unhandled help shortcut ${helpItem.id}`);
    }
  }

  private async deleteMessage(): Promise<void> {
    if (!this.queueName) {
      throw new Error("invalid state, missing queueName");
    }
    if (!this.messageId) {
      throw new Error("invalid state, missing messageId");
    }

    const app = App.getApp(this);

    const confirm = await app.confirmDialog.show({
      title: "Delete Queue",
      message: `Are you sure you want to delete ${this.messageId}?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (confirm === "Delete") {
      try {
        await app.api.api.deleteMessage(this.queueName, this.messageId);
        StatusBar.setStatus(this.document, `${this.messageId} deleted!`);
        this.window.history.popState();
      } catch (err) {
        logger.error(`Failed to delete message`, err);
        StatusBar.setStatus(this.document, `Failed to delete message`, err);
      }
    }
  }
}

function attributesToString(attributes: Record<string, string>): string {
  return Object.keys(attributes)
    .map((key) => `${key}=${attributes[key]}`)
    .join("\n");
}

function bodyToString(body: string): string {
  try {
    const json = JSON.parse(body) as unknown;
    return JSON.stringify(json, null, 2);
  } catch (_err) {
    return body;
  }
}
