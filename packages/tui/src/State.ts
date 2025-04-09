import EventEmitter from "node:events";

export interface StateEvents {
    'changed': () => void;
}

export enum Screen {
    Queues
}

export class State extends EventEmitter {
    private _screen: Screen = Screen.Queues;

    public override on<T extends keyof StateEvents>(event: T, listener: StateEvents[T]): this {
        return super.on(event, listener);
    }

    public override emit<T extends keyof StateEvents>(event: T, ...args: Parameters<StateEvents[T]>): boolean {
        return super.emit(event, ...args);
    }

    public get screen(): Screen {
        return this._screen;
    }

    public set screen(screen: Screen) {
        this.screen = screen;
        this.emit('changed');
    }
}
