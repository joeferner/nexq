import { Key } from "readline";
import { Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { isInputMatch } from "../utils/input.js";
import { Button } from "./Button.js";
import { Dialog } from "./Dialog.js";
import { Text } from "./Text.js";

export interface ConfirmOptions {
  title: string;
  message: string;
  options: string[];
  defaultOption: string;
}

export class ConfirmDialog extends Dialog<ConfirmOptions, string | undefined> {
  public static readonly ID = "confirmDialog";

  public constructor(state: NexqState) {
    super(state, ConfirmDialog.ID);
  }

  protected override handleKeyPress(chunk: string, key: Key | undefined): void {
    if (isInputMatch(key, "escape")) {
      this.close(undefined);
    } else if (isInputMatch(key, "return")) {
      const focusedComponent = this.box.getFocusedChild();
      if (focusedComponent instanceof Button) {
        this.close(focusedComponent.id);
      } else {
        this.state.setStatus("Failed to get focused option");
      }
    } else {
      super.handleKeyPress(chunk, key);
      this.state.emit("changed");
    }
  }

  public override show(options: ConfirmOptions): Promise<string | undefined> {
    this.title = options.title;

    const optionsContainer = new Component();
    let focusedOption: Component | undefined;
    optionsContainer.width = "100%";
    optionsContainer.justifyContent = Justify.Center;
    for (let i = 0; i < options.options.length; i++) {
      const button = new Button({ text: `  ${options.options[i]}  ` });
      button.id = options.options[i];
      button.tabIndex = i + 1;
      optionsContainer.children.push(button);
      if (options.options[i] === options.defaultOption) {
        focusedOption = button;
      }
    }
    this.setFocusedChild(focusedOption ?? optionsContainer.children[0]);

    const message = new Text({ text: options.message });
    message.margin = { bottom: 1 };

    this.box.children = [message, optionsContainer];

    return super.show(options);
  }
}
