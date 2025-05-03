import matchPath from "node-match-path";
import { Document } from "./Document.js";
import { Element } from "./Element.js";

export interface Route {
  pathname: string;
  element: Element;
}

export interface RouterElementOptions {
  routes: Route[];
}

export class RouterElement extends Element {
  private readonly boundUpdateChild = this.updateChild.bind(this);
  private _matchingRoute?: Route;

  public constructor(
    document: Document,
    private readonly options: RouterElementOptions
  ) {
    super(document);
  }

  protected override elementDidMount(): void {
    this.updateChild();
    this.window.addEventListener("popstate", this.boundUpdateChild);
    this.window.addEventListener("pushstate", this.boundUpdateChild);
  }

  protected override elementWillUnmount(): void {
    this.window.removeEventListener("popstate", this.boundUpdateChild);
    this.window.removeEventListener("pushstate", this.boundUpdateChild);
  }

  private updateChild(): void {
    const newMatchingRoute = this.findMatchingRoute();
    if (newMatchingRoute && newMatchingRoute !== this._matchingRoute) {
      this._matchingRoute = newMatchingRoute;
      while (this.lastElementChild) {
        this.removeChild(this.lastElementChild);
      }
      this.appendChild(newMatchingRoute.element);
      void this.window.refresh();
    }
  }

  private findMatchingRoute(): Route | undefined {
    const location = this.window.location;
    for (const route of this.options.routes) {
      if (isPathMatch(route.pathname, location.pathname)) {
        return route;
      }
    }
    return undefined;
  }
  public get matchingRoute(): Route | undefined {
    return this._matchingRoute;
  }
}

export function isPathMatch(expr: string, pathname: string): boolean {
  return matchPath.match(expr, pathname).matches;
}
