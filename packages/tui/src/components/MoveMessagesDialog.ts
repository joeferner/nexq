import { Key } from "readline";
import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { isInputMatch } from "../utils/input.js";
import { Button } from "./Button.js";
import { Dialog } from "./Dialog.js";
import { InputBox } from "./InputBox.js";
import { Text } from "./Text.js";

export interface MoveMessagesDialogOptions {
  sourceQueueName: string;
}

export interface MoveMessagesDialogResults {
  targetQueueName: string;
}

export class MoveMessagesDialog extends Dialog<MoveMessagesDialogOptions, MoveMessagesDialogResults | undefined> {
  public static readonly ID = "moveMessagesDialog";
  private readonly message = new Text({ text: "" });
  private readonly inputBox: InputBox;
  private readonly cancelButton: Button;
  private readonly moveButton: Button;

  public constructor(state: NexqState) {
    super(state, MoveMessagesDialog.ID);

    this.inputBox = new InputBox({
      width: 40,
      inputBoxFocusColor: state.inputBoxFocusColor,
      inputBoxFocusBgColor: state.inputBoxFocusBgColor,
    });
    this.inputBox.tabIndex = 1;
    this.inputBox.margin = { bottom: 1 };

    this.cancelButton = new Button({ text: "  Cancel  " });
    this.cancelButton.tabIndex = 2;

    this.moveButton = new Button({ text: "  Move  " });
    this.moveButton.tabIndex = 3;

    this.message.margin = { bottom: 1 };

    this.title = "Move Messages";

    const inputContainer = new Component();
    inputContainer.flexDirection = FlexDirection.Row;
    inputContainer.children = [new Text({ text: "To: " }), this.inputBox];

    const optionsContainer = new Component();
    optionsContainer.width = "100%";
    optionsContainer.justifyContent = Justify.Center;
    optionsContainer.children.push(this.cancelButton);
    optionsContainer.children.push(this.moveButton);

    this.box.children = [this.message, inputContainer, optionsContainer];
  }

  public override show(options: MoveMessagesDialogOptions): Promise<MoveMessagesDialogResults | undefined> {
    this.inputBox.value = "";
    this.message.text = `Move messages from "${options.sourceQueueName}"`;
    this.setFocusedChild(this.inputBox);
    return super.show(options);
  }

  protected override handleKeyPress(chunk: string, key: Key | undefined): void {
    if (isInputMatch(key, "escape")) {
      this.close(undefined);
      return;
    }

    if (this.inputBox.focused && this.inputBox.handleKeyPress(chunk, key)) {
      this.state.emit("changed");
      return;
    }

    if (this.cancelButton.focused && isInputMatch(key, "return")) {
      this.close(undefined);
      return;
    }

    if ((this.inputBox.focused || this.moveButton.focused) && isInputMatch(key, "return")) {
      this.close({
        targetQueueName: this.inputBox.value,
      });
      return;
    }

    super.handleKeyPress(chunk, key);
    this.state.emit("changed");
  }
}
