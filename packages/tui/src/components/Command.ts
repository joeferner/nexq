import { Align, Display, FlexDirection } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { DivElement } from "../render/DivElement.js";
import { Document } from "../render/Document.js";
import { Element } from "../render/Element.js";
import { InputBox } from "../render/InputBox.js";
import { KeyboardEvent } from "../render/KeyboardEvent.js";
import { isInputMatch } from "../utils/input.js";

export class Command extends DivElement {
  private readonly input: InputBox;
  private lastFocusElement?: Element;
  public onCommand: (value: string) => boolean = () => true;

  public constructor(document: Document) {
    super(document);
    this.style.width = "100%";
    this.style.flexGrow = 1;
    this.style.alignItems = Align.Stretch;
    this.style.flexDirection = FlexDirection.Row;
    this.style.display = Display.None;
    this.style.borderStyle = "solid";
    this.style.borderColor = NexqStyles.borderColor;

    this.input = new InputBox(document);
    NexqStyles.applyToInputBox(this.input);
    this.input.style.width = "100%";
    this.appendChild(this.input);
  }

  public show(): void {
    this.input.value = "";
    this.lastFocusElement = this.window.activeElement ?? undefined;
    this.style.display = Display.Flex;
    this.input.focus();
    void this.window.refresh();
  }

  protected override onKeyDown(event: KeyboardEvent): void {
    if (isInputMatch(event, "escape")) {
      this.style.display = Display.None;
      this.lastFocusElement?.focus();
      return;
    }

    if (isInputMatch(event, "return")) {
      if (this.onCommand(this.input.value)) {
        this.style.display = Display.None;
        this.lastFocusElement?.focus();
      }
      return;
    }

    super.onKeyDown(event);
  }
}
