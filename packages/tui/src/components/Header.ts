import { BoxComponent } from "../render/BoxComponent.js";
import { Component } from "../render/Renderer.js";
import { TextComponent } from "../render/TextComponent.js";

const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export class Header extends Component {
    private readonly children: Component[] = [new BoxComponent({
        children: [
            new TextComponent({ text: LOGO, color: '#fca321' })
        ]
    })];

    public getChildren(): Component[] {
        return this.children;
    }
}
