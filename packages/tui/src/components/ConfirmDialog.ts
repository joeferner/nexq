import { Key } from "readline";
import { NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection, JustifyContent } from "../render/BoxComponent.js";
import { TextComponent } from "../render/TextComponent.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { Dialog } from "./Dialog.js";

const logger = createLogger("ConfirmDialog");

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

  protected override handleKeyPress(_chunk: string, key: Key | undefined): void {
    if (isInputMatch(key, "escape")) {
      this.close(undefined);
    } else if (isInputMatch(key, "left")) {
      this.updateSelectedOption(-1);
    } else if (isInputMatch(key, "right") || isInputMatch(key, "tab")) {
      this.updateSelectedOption(1);
    } else if (isInputMatch(key, "return")) {
      this.close(this.selectedOption);
    } else {
      logger.debug("unhandled key press", JSON.stringify(key));
    }
  }

  public get selectedOption(): string | undefined {
    if (this.state.focus === ConfirmDialog.ID) {
      return this.options?.defaultOption;
    }

    const prefix = `${ConfirmDialog.ID}:`;
    if (this.state.focus.startsWith(prefix)) {
      return this.state.focus.substring(prefix.length);
    }
    return undefined;
  }

  public set selectedOption(option: string | undefined) {
    if (option) {
      this.state.focus = `${ConfirmDialog.ID}:${option}`;
    }
  }

  private updateSelectedOption(dir: number): void {
    if (!this.options) {
      return;
    }

    let i = this.options.options.indexOf(this.selectedOption ?? "") ?? 0;
    i += dir;
    i = i % this.options.options.length;
    this.selectedOption = this.options.options[i];
    this.refresh();
  }

  protected override refresh(): void {
    if (!this.options) {
      return;
    }

    const { options, message, title } = this.options;
    const optionsWidth = options.reduce((p, o) => p + o.length + 3, 0);
    this.update({
      title,
      children: [
        new BoxComponent({
          direction: BoxDirection.Vertical,
          width: message.length + 4,
          height: 5,
          justifyContent: JustifyContent.Center,
          children: [
            new TextComponent({
              text: `\n${message}\n`,
            }),
            new BoxComponent({
              width: optionsWidth,
              direction: BoxDirection.Horizontal,
              justifyContent: JustifyContent.SpaceBetween,
              children: options.map((option) => {
                return new TextComponent({ text: ` ${option} `, inverse: this.selectedOption === option });
              }),
            }),
          ],
        }),
      ],
    });
  }
}
