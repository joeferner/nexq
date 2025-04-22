export class Location {
  private _pathname: string = "";
  private _href: string = "";

  public constructor(url: URL) {
    this.setUrl(url);
  }

  public get pathname(): string {
    return this._pathname;
  }

  public get href(): string {
    return this._href;
  }

  private setUrl(url: URL): void {
    this._pathname = url.pathname;
    this._href = url.href;
  }
}

export interface EditableLocation {
  setUrl(url: URL): void;
}
