import { FlexDirection, Justify } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { Button } from "../render/Button.js";
import { Dialog } from "../render/Dialog.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { InputBox } from "../render/InputBox.js";
import { Text } from "../render/Text.js";
import { fgColor } from "../render/color.js";

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
    super(document);
    this.id = "MoveMessagesDialog";
    NexqStyles.applyToDialog(this);

    this.inputBox = new InputBox(document);
    NexqStyles.applyToInputBox(this.inputBox);
    this.inputBox.style.width = 40;
    this.inputBox.tabIndex = 1;
    this.inputBox.style.marginBottom = 1;

    this.cancelButton = new Button(document);
    this.cancelButton.text = "  Cancel  ";
    NexqStyles.applyToButton(this.cancelButton);
    this.cancelButton.tabIndex = 2;
    this.cancelButton.addEventListener("click", () => {
      this.close(undefined);
    });

    this.moveButton = new Button(document);
    this.moveButton.text = "  Move  ";
    NexqStyles.applyToButton(this.moveButton);
    this.moveButton.tabIndex = 3;
    this.moveButton.addEventListener("click", () => {
      this.close({
        targetQueueName: this.inputBox.value,
      });
    });

    this.message.style.marginBottom = 1;

    this.borderTitle = fgColor(NexqStyles.dialogTitleColor)`<Move Messages>`;

    const inputContainer = new DivElement(document);
    inputContainer.style.flexDirection = FlexDirection.Row;
    inputContainer.appendChild(new Text(document, { text: "To: " }));
    inputContainer.appendChild(this.inputBox);

    const optionsContainer = new DivElement(document);
    optionsContainer.style.width = "100%";
    optionsContainer.style.justifyContent = Justify.Center;
    optionsContainer.appendChild(this.cancelButton);
    optionsContainer.appendChild(this.moveButton);

    this.appendChild(this.message);
    this.appendChild(inputContainer);
    this.appendChild(optionsContainer);
  }

  public override async onShow(options: MoveMessagesDialogOptions): Promise<void> {
    this.inputBox.value = "";
    this.message.text = `Move messages from "${options.sourceQueueName}"`;
    this.inputBox.focus();
    return super.onShow(options);
  }
}
