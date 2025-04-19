import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { Component } from "../render/Component.js";
import { TextComponent } from "../render/TextComponent.js";
import { Help } from "./Help.js";

const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export class Header extends Component {
  public constructor(state: NexqState) {
    super();
    this.flexGrow = 0;

    const createLeftItem = (name: string, value: string): Component => {
      const item = new Component();
      item.children = [
        new TextComponent({ text: name, color: state.headerNameColor }),
        new TextComponent({ text: value, color: state.headerValueColor }),
      ];
      return item;
    };

    const left = new Component();
    left.flexDirection = FlexDirection.Column;
    left.justifyContent = Justify.FlexEnd;
    left.height = '100%';
    left.children.push(createLeftItem("TUI Ver:  ", `v${state.tuiVersion}`));
    left.children.push(createLeftItem("NexQ Ver: ", `v${state.nexqVersion}`));

    this.flexDirection = FlexDirection.Row;
    this.justifyContent = Justify.SpaceBetween;
    this.width = '100%';
    this.height = 5;

    this.children.push(left);
    this.children.push(new Help(state));
    this.children.push(new TextComponent({ text: LOGO, color: state.logoColor }));
  }
}
