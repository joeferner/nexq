export interface KeyboardEventOptions {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  key: string;
}

export class KeyboardEvent {
  public readonly altKey: boolean;
  public readonly ctrlKey: boolean;
  public readonly metaKey: boolean;
  public readonly shiftKey: boolean;
  public readonly key: string;

  public constructor(options: KeyboardEventOptions) {
    this.altKey = options.altKey;
    this.ctrlKey = options.ctrlKey;
    this.metaKey = options.metaKey;
    this.shiftKey = options.shiftKey;
    this.key = options.key;
  }
}
