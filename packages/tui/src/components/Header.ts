import { FlexDirection, Justify } from "yoga-layout";
import { NexqState } from "../NexqState.js";
import { BoxComponent } from "../render/BoxComponent.js";
import { Component } from "../render/Component.js";
import { TextComponent } from "../render/TextComponent.js";
import { Help } from "./Help.js";

const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export class Header extends Component {
  private readonly _children: Component[];

  public constructor(state: NexqState) {
    super();
    this.flexGrow = 0;

    const createLeftItem = (name: string, value: string): BoxComponent => {
      return new BoxComponent({
        direction: FlexDirection.Row,
        children: [
          new TextComponent({ text: name, color: state.headerNameColor }),
          new TextComponent({ text: value, color: state.headerValueColor }),
        ],
      });
    };

    const left = new BoxComponent({
      direction: FlexDirection.Column,
      justifyContent: Justify.FlexEnd,
      children: [
        createLeftItem("TUI Ver:  ", `v${state.tuiVersion}`),
        createLeftItem("NexQ Ver: ", `v${state.nexqVersion}`),
      ],
    });

    this._children = [
      new BoxComponent({
        direction: FlexDirection.Row,
        justifyContent: Justify.SpaceBetween,
        width: '100%',
        height: 5,
        children: [left, new Help(state), new TextComponent({ text: LOGO, color: state.logoColor })],
      }),
    ];
  }

  public get children(): Component[] {
    return this._children;
  }
}
