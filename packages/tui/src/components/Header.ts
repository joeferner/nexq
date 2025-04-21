import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { Text } from "./Text.js";
import { Help } from "./Help.js";

const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export class Header extends Component {
  public constructor(state: NexqState) {
    super();
    this.style.flexGrow = 0;

    const createLeftItem = (name: string, value: string): Component => {
      const item = new Component();
      item.children = [
        new Text({ text: name, color: state.headerNameColor }),
        new Text({ text: value, color: state.headerValueColor }),
      ];
      return item;
    };

    const left = new Component();
    left.style.flexDirection = FlexDirection.Column;
    left.style.justifyContent = Justify.FlexEnd;
    left.style.height = "100%";
    left.children.push(createLeftItem("TUI Ver:  ", `v${state.tuiVersion}`));
    left.children.push(createLeftItem("NexQ Ver: ", `v${state.nexqVersion}`));

    this.style.flexDirection = FlexDirection.Row;
    this.style.justifyContent = Justify.SpaceBetween;
    this.style.width = "100%";
    this.style.height = 5;

    this.children.push(left);
    this.children.push(new Help(state));
    this.children.push(new Text({ text: LOGO, color: state.logoColor }));
  }
}
