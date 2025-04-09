import { NexqApi } from "../NexqApi.js";
import { BoxComponent } from "../render/BoxComponent.js";
import { Component } from "../render/Renderer.js";
import { Header } from "./Header.js";

export class App extends Component {
    private readonly header = new Header();
    private readonly children: Component[] = [new BoxComponent({ children: [this.header] })];

    public constructor(private readonly api: NexqApi) {
        super();
    }

    public getChildren(): Component[] {
        return this.children;
    }
}
