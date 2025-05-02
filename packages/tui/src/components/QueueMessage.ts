import { hex } from "ansis";
import matchPath from "node-match-path";
import { Align, FlexDirection } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { Box } from "../render/Box.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { BorderType } from "../render/RenderItem.js";
import { Text } from "../render/Text.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { App } from "./App.js";
import { HelpItem } from "./Help.js";
import { StatusBar } from "./StatusBar.js";
import { PeekMessagesResponseMessage } from "../client/NexqClientApi.js";
import { Detail, detailsToString } from "./details.js";

const logger = createLogger("QueueMessage");

export class QueueMessage extends Element {
  public static readonly PATH = "/queue/:queueName/:messageId";
  private readonly box: Box;
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

    this.text = new Text(document, { text: "" });
    this.text.style.flexGrow = 1;
    this.box.appendChild(this.text);
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
      { title: "Sent Time", value: message.sentTime },
      { title: "Last Nak", value: message.lastNakReason ?? "<not set>" },
      { title: "Attributes", value: attributesToString(message.attributes) },
      { title: "Body", value: bodyToString(message.body), newLineNoIndent: true },
    ];
    this.text.text = detailsToString(details);

    this.box.title = hex(NexqStyles.titleColor)` Message(` + hex(NexqStyles.titleAltColor)`${this.queueName}/${this.messageId}` + hex(NexqStyles.titleColor)`) `;

    const run = async (): Promise<void> => {
      await this.window.refresh();
      if (this.window.activeElement !== this.box) {
        this.box.focus();
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
