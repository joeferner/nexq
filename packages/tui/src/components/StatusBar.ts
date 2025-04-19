import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { BoxComponent } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { TextComponent } from "../render/TextComponent.js";

export class StatusBar extends Component {
  private _children: Component[];
  private textComponent = new TextComponent({ text: "" });
  private clearStatusTimeout?: NodeJS.Timeout;

  public constructor(state: NexqState) {
    super();
    this._children = [
      new BoxComponent({
        children: [this.textComponent],
        direction: FlexDirection.Column,
        justifyContent: Justify.Center,
        width: "100%",
      }),
    ];
    state.on("changed", () => {
      if (this.clearStatusTimeout) {
        clearTimeout(this.clearStatusTimeout);
      }
      this.textComponent.text = state.status;
      this.clearStatusTimeout = setTimeout(() => {
        state.setStatus("");
      }, state.statusTimeout);
    });
  }

  public get children(): Component[] {
    return this._children;
  }
}
