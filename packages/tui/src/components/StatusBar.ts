import { Align, FlexDirection } from "yoga-layout";
import { Element } from "../render/Element.js";
import { getErrorMessage } from "../utils/error.js";
import { Text } from "../render/Text.js";
import { Document } from "../render/Document.js";

export class StatusBar extends Element {
  public static readonly ID = "NexqStatusBar";
  private textElement = new Text(this.document, { text: "" });
  private clearStatusTimeout?: NodeJS.Timeout;
  private statusTimeout = 3 * 1000;

  public constructor(document: Document) {
    super(document);
    this.id = StatusBar.ID;
    this.style.flexDirection = FlexDirection.Column;
    this.style.alignItems = Align.Center;
    this.style.width = "100%";
    this.style.height = 1;
    this.style.minHeight = 1;
    this.style.maxHeight = 1;
    this.appendChild(this.textElement);
  }

  public setStatus(message: string, err?: unknown): void {
    let statusMessage = message;
    if (err) {
      statusMessage += ` ${getErrorMessage(err)}`;
    }
    if (statusMessage !== this.textElement.text) {
      if (this.clearStatusTimeout) {
        clearTimeout(this.clearStatusTimeout);
      }
      this.textElement.text = statusMessage;
      this.clearStatusTimeout = setTimeout(() => {
        this.textElement.text = "";
      }, this.statusTimeout);
    }
  }

  public static setStatus(document: Document, message: string, err?: unknown): void {
    const statusBar = document.getElementById(StatusBar.ID) as StatusBar | null;
    statusBar?.setStatus(message, err);
  }
}
