import { Align, FlexDirection } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { Text } from "./Text.js";

export class StatusBar extends Component {
  private textComponent = new Text({ text: "" });
  private clearStatusTimeout?: NodeJS.Timeout;

  public constructor(state: NexqState) {
    super();
    this.flexDirection = FlexDirection.Column;
    this.alignItems = Align.Center;
    this.width = "100%";
    this.children.push(this.textComponent);

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
}
