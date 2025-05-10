import { Justify } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { Button } from "../render/Button.js";
import { Dialog } from "../render/Dialog.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { Text } from "../render/Text.js";
import { fgColor } from "../render/color.js";
import { isInputMatch } from "../utils/input.js";
import { StatusBar } from "./StatusBar.js";

export interface ConfirmOptions {
  title: string;
  message: string;
  options: string[];
  defaultOption: string;
}

export class ConfirmDialog extends Dialog<ConfirmOptions, string | undefined> {
  public constructor(document: Document) {
    super(document);
    this.id = "ConfirmDialog";
    NexqStyles.applyToDialog(this);
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "escape")) {
      this.close(undefined);
    } else if (isInputMatch(event, "return")) {
      const focusedElement = this.activeElement;
      if (focusedElement instanceof Button) {
        this.close(focusedElement.id);
      } else {
        StatusBar.setStatus(this.document, "Failed to get focused option");
      }
    } else {
      super.onKeyDown(event);
    }
  }

  public override async onShow(options: ConfirmOptions): Promise<void> {
    this.borderTitle = fgColor(NexqStyles.dialogTitleColor)`<${options.title}>`;

    while (this.lastElementChild) {
      this.removeChild(this.lastElementChild);
    }

    const optionsContainer = new DivElement(this.document);
    let focusedOption: Element | undefined;
    optionsContainer.style.width = "100%";
    optionsContainer.style.justifyContent = Justify.Center;
    for (let i = 0; i < options.options.length; i++) {
      const button = new Button(this.document);
      button.text = `  ${options.options[i]}  `;
      NexqStyles.applyToButton(button);
      button.id = options.options[i];
      button.tabIndex = i + 1;
      button.addEventListener("click", () => {
        this.close(button.id);
      });
      optionsContainer.appendChild(button);
      if (options.options[i] === options.defaultOption) {
        focusedOption = button;
      }
    }

    const message = new Text(this.document, { text: options.message });
    message.style.marginBottom = 1;

    this.appendChild(message);
    this.appendChild(optionsContainer);

    const focusedChild = focusedOption ?? optionsContainer.firstElementChild;
    focusedChild?.focus();

    return super.onShow(options);
  }
}
