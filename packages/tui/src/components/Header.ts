import { FlexDirection, Justify } from "yoga-layout";
import { NexqStyles } from "../NexqStyles.js";
import { Element } from "../render/Element.js";
import { Document } from "../render/Document.js";
import { Help } from "./Help.js";
import { Text } from "../render/Text.js";
import { App } from "./App.js";
import { DivElement } from "../render/DivElement.js";

const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export class Header extends Element {
  private readonly tuiVersion: Text;
  private readonly nexqVersion: Text;

  public constructor(document: Document) {
    super(document);
    this.id = "header";
    this.style.flexGrow = 0;

    const createLeftItem = (name: string, valueElement: Text): Element => {
      const item = new DivElement(document);
      item.appendChild(new Text(document, { text: name, color: NexqStyles.headerNameColor }));
      item.appendChild(valueElement);
      return item;
    };

    this.tuiVersion = new Text(document, { text: "v???", color: NexqStyles.headerValueColor });
    this.nexqVersion = new Text(document, { text: "v???", color: NexqStyles.headerValueColor });

    const left = new DivElement(document);
    left.style.flexDirection = FlexDirection.Column;
    left.style.justifyContent = Justify.FlexEnd;
    left.style.height = "100%";
    left.appendChild(createLeftItem("TUI Ver:  ", this.tuiVersion));
    left.appendChild(createLeftItem("NexQ Ver: ", this.nexqVersion));

    this.style.flexDirection = FlexDirection.Row;
    this.style.justifyContent = Justify.SpaceBetween;
    this.style.width = "100%";
    this.style.height = 5;

    this.appendChild(left);
    this.appendChild(new Help(document));

    const logo = new Text(document);
    logo.text = LOGO;
    logo.style.color = NexqStyles.logoColor;
    this.appendChild(logo);
  }

  protected override elementDidMount(): void {
    const app = App.getApp(this);

    this.tuiVersion.text = `v${app.tuiVersion}`;

    void this.loadInfo(app);
    void this.window.refresh();
  }

  private async loadInfo(app: App): Promise<void> {
    const nexqVersion = await app.nexqVersion;
    this.nexqVersion.text = `v${nexqVersion}`;
    await this.window.refresh();
  }
}
