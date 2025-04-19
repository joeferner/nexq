import { Key } from "readline";
import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { BoxComponent } from "../render/BoxComponent.js";
import { TextComponent } from "../render/TextComponent.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { Dialog } from "./Dialog.js";
import { InputBox } from "./InputBox.js";

const logger = createLogger("MoveMessagesDialog");

export interface MoveMessagesDialogOptions {
  sourceQueueName: string;
}

export interface MoveMessagesDialogResults {
  targetQueueName: string;
}

export class MoveMessagesDialog extends Dialog<MoveMessagesDialogOptions, MoveMessagesDialogResults | undefined> {
  public static readonly ID = "moveMessagesDialog";
  private readonly inputBox: InputBox;
  private readonly cancelButton = new TextComponent({ text: "  Cancel  " });
  private readonly moveButton = new TextComponent({ text: "  Move  " });

  public constructor(state: NexqState) {
    super(state, MoveMessagesDialog.ID);
    this.inputBox = new InputBox(state, { width: 40 });
  }

  public show(options: MoveMessagesDialogOptions): Promise<MoveMessagesDialogResults | undefined> {
    this.inputBox.value = "";
    return super.show(options);
  }

  protected override handleKeyPress(_chunk: string, key: Key | undefined): void {
    if (isInputMatch(key, "tab")) {
      this.focusNext();
      this.state.emit("changed");
    } else if (isInputMatch(key, "escape")) {
      this.close(undefined);
    } else if (isInputMatch(key, "right") || isInputMatch(key, "left")) {
      const focusedId = this.focusedControlId;
      if (focusedId === "cancel") {
        this.state.focus = `${MoveMessagesDialog.ID}:move`;
        this.updateFocus();
        this.state.emit("changed");
      } else if (focusedId === "move") {
        this.state.focus = `${MoveMessagesDialog.ID}:cancel`;
        this.updateFocus();
        this.state.emit("changed");
      }
    } else if (isInputMatch(key, "up")) {
      const focusedId = this.focusedControlId;
      if (focusedId === "cancel" || focusedId === "move") {
        this.state.focus = `${MoveMessagesDialog.ID}:input`;
        this.updateFocus();
        this.state.emit("changed");
      }
    } else if (isInputMatch(key, "return")) {
      const focusedId = this.focusedControlId;
      if (focusedId === "move" || focusedId === "input") {
        this.close({
          targetQueueName: this.inputBox.value,
        });
      } else {
        this.close(undefined);
      }
    } else {
      logger.info("unhandled key", JSON.stringify(key));
    }
  }

  private get focusedControlId(): string {
    const id = this.state.focus.substring(MoveMessagesDialog.ID.length + ":".length);
    if (id.length > 0) {
      return id;
    }
    return "move";
  }

  private focusNext(): void {
    const focusedId = this.focusedControlId;
    if (focusedId === "input") {
      this.state.focus = `${MoveMessagesDialog.ID}:cancel`;
    } else if (focusedId === "cancel") {
      this.state.focus = `${MoveMessagesDialog.ID}:move`;
    } else {
      this.state.focus = `${MoveMessagesDialog.ID}:input`;
    }

    this.updateFocus();
  }

  private updateFocus(): void {
    const focusedId = this.focusedControlId;
    this.inputBox.focus = false;
    this.cancelButton.inverse = false;
    this.moveButton.inverse = false;
    if (focusedId === "input") {
      this.inputBox.focus = true;
    } else if (focusedId === "cancel") {
      this.cancelButton.inverse = true;
    } else {
      this.moveButton.inverse = true;
    }
  }

  protected refresh(): void {
    if (!this.options) {
      return;
    }

    this.focusNext();

    const message = ` Move messages from "${this.options.sourceQueueName}" `;

    this.update({
      title: "Move Messages",
      children: [
        new BoxComponent({
          direction: FlexDirection.Column,
          width: Math.max(" To: ".length + this.inputBox.width + 1, message.length),
          height: 7,
          justifyContent: Justify.Center,
          children: [
            new TextComponent({
              text: `\n${message}\n`,
            }),
            new BoxComponent({
              direction: FlexDirection.Row,
              justifyContent: Justify.FlexStart,
              children: [new TextComponent({ text: " To: " }), this.inputBox],
            }),
            new TextComponent({
              text: ``,
            }),
            new BoxComponent({
              width: this.cancelButton.text.length + this.moveButton.text.length,
              direction: FlexDirection.Row,
              justifyContent: Justify.SpaceBetween,
              children: [this.cancelButton, this.moveButton],
            }),
          ],
        }),
      ],
    });
  }
}
