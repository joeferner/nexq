import { ConfirmOptions, NexqState } from "../NexqState.js";
import { BoxComponent, BoxDirection, JustifyContent } from "../render/BoxComponent.js";
import { TextComponent } from "../render/TextComponent.js";
import { isInputMatch } from "../utils/input.js";
import { createLogger } from "../utils/logger.js";
import { Dialog } from "./Dialog.js";

const logger = createLogger("ConfirmDialog");

export class ConfirmDialog extends Dialog {
  public static readonly ID = "confirmDialog";
  private lastConfirmOptions?: ConfirmOptions;
  private selectedOption?: string;

  public constructor(state: NexqState) {
    super(state);
    state.on("changed", () => {
      if (state.confirmOptions !== this.lastConfirmOptions) {
        this.lastConfirmOptions = state.confirmOptions;
        if (state.confirmOptions) {
          this.selectedOption = state.confirmOptions.defaultOption;
          this.refresh();
        } else {
          this.hide();
        }
      }
    });
    state.on("keypress", (_chunk, key) => {
      if (state.focus !== ConfirmDialog.ID) {
        return;
      }
      if (isInputMatch(key, "escape")) {
        state.exitConfirmDialog(undefined);
      } else if (isInputMatch(key, "left")) {
        this.updateSelectedOption(-1);
      } else if (isInputMatch(key, "right") || isInputMatch(key, "tab")) {
        this.updateSelectedOption(1);
      } else if (isInputMatch(key, "return")) {
        state.exitConfirmDialog(this.selectedOption);
      } else {
        logger.debug("unhandled key press", JSON.stringify(key));
      }
    });
  }

  private updateSelectedOption(dir: number): void {
    const { confirmOptions } = this.state;
    if (!confirmOptions) {
      return;
    }

    let i = confirmOptions.options.indexOf(this.selectedOption ?? "") ?? 0;
    i += dir;
    i = i % confirmOptions.options.length;
    this.selectedOption = confirmOptions.options[i];
    this.refresh();
  }

  private refresh(): void {
    const { confirmOptions } = this.state;
    if (!confirmOptions) {
      return;
    }

    const optionsWidth = confirmOptions.options.reduce((p, o) => p + o.length + 3, 0);
    this.update({
      title: confirmOptions.title,
      children: [
        new BoxComponent({
          direction: BoxDirection.Vertical,
          width: confirmOptions.message.length + 4,
          height: 5,
          justifyContent: JustifyContent.Center,
          children: [
            new TextComponent({
              text: `\n${confirmOptions.message}\n`,
            }),
            new BoxComponent({
              width: optionsWidth,
              direction: BoxDirection.Horizontal,
              justifyContent: JustifyContent.SpaceBetween,
              children: confirmOptions.options.map((option) => {
                return new TextComponent({ text: ` ${option} `, inverse: this.selectedOption === option });
              }),
            }),
          ],
        }),
      ],
    });
  }
}
