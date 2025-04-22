import { Element } from "./Element.js";
import { Window } from "./Window.js";

export class Document extends Element {
  public constructor() {
    super(undefined as unknown as Document);
  }

  public override get document(): Document {
    return this;
  }

  public override get window(): Window {
    return this.parentElement as Window;
  }

  public get documentElement(): Element | null {
    return this.firstElementChild;
  }

  public getElementById(id: string): Element | null {
    let found: Element | null = null;
    this.walkChildrenDeep((child) => {
      if (child.id === id) {
        found = child;
      }
    });
    return found;
  }
}
