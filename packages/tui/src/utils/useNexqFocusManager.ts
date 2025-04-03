import { useFocusManager } from "ink";
import React from "react";

export interface NexqFocusManager {
    activeId: string | null;
    focus: (id: string) => void;
    enableFocus: () => void;
}

export function useNexqFocusManager(): NexqFocusManager {
    const [activeId, setActiveId] = React.useState<string | null>(null);
    const { focus, enableFocus } = useFocusManager();

    const myFocus = (id: string): void => {
        focus(id);
        setActiveId(id);
    };

    return {
        activeId,
        focus: myFocus,
        enableFocus
    }
}