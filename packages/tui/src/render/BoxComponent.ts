import { Component } from "./Renderer.js";

export interface BoxComponentOptions {
    children: Component[];
}

export class BoxComponent extends Component {
    public constructor(private readonly options: BoxComponentOptions) {
        super();
    }

    public getChildren(): Component[] {
        return this.options.children;
    }
}
