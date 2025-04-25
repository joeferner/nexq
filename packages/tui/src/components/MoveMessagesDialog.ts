import { FlexDirection, Justify } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { Element } from "../render/Element.js";
import { Document } from "../render/Document.js";
import { Button } from "../render/Button.js";
import { Dialog } from "../render/Dialog.js";
import { InputBox } from "../render/InputBox.js";
import { Text } from "../render/Text.js";

export interface MoveMessagesDialogOptions {
  sourceQueueName: string;
}

export interface MoveMessagesDialogResults {
  targetQueueName: string;
}

export class MoveMessagesDialog extends Dialog<MoveMessagesDialogOptions, MoveMessagesDialogResults> {
  private readonly message = new Text(this.document, { text: "" });
  private readonly inputBox: InputBox;
  private readonly cancelButton: Button;
  private readonly moveButton: Button;

  public constructor(document: Document) {
    super(document, { ...NexqStyles.dialogStyles });

    this.inputBox = new InputBox(document, {
      width: 40,
      ...NexqStyles.inputStyles,
    });
    this.inputBox.tabIndex = 1;
    this.inputBox.style.marginBottom = 1;

    this.cancelButton = new Button(document, { text: "  Cancel  ", ...NexqStyles.buttonStyles });
    this.cancelButton.tabIndex = 2;
    this.cancelButton.addEventListener("click", () => {
      this.close(undefined);
    });

    this.moveButton = new Button(document, { text: "  Move  ", ...NexqStyles.buttonStyles });
    this.moveButton.tabIndex = 3;
    this.moveButton.addEventListener("click", () => {
      this.close({
        targetQueueName: this.inputBox.value,
      });
    });

    this.message.style.marginBottom = 1;

    this.title = "Move Messages";

    const inputContainer = new Element(document);
    inputContainer.style.flexDirection = FlexDirection.Row;
    inputContainer.appendChild(new Text(document, { text: "To: " }));
    inputContainer.appendChild(this.inputBox);

    const optionsContainer = new Element(document);
    optionsContainer.style.width = "100%";
    optionsContainer.style.justifyContent = Justify.Center;
    optionsContainer.appendChild(this.cancelButton);
    optionsContainer.appendChild(this.moveButton);

    this.box.appendChild(this.message);
    this.box.appendChild(inputContainer);
    this.box.appendChild(optionsContainer);
  }

  public override async onShow(options: MoveMessagesDialogOptions): Promise<void> {
    this.inputBox.value = "";
    this.message.text = `Move messages from "${options.sourceQueueName}"`;
    this.inputBox.focus();
    return super.onShow(options);
  }
}
