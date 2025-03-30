import { useFocusManager } from "ink";
import React, { createContext } from "react";

export interface NexqFocusManager {
  activeId: string | null;
  focus: (id: string) => void;
  enableFocus: () => void;
}

interface ContextData {
  activeId: string | null;
}

export const NexqFocusManagerContext = createContext<ContextData>({ activeId: null });

export function useNexqFocusManager(): NexqFocusManager {
  const ctx = React.useContext(NexqFocusManagerContext);
  const { focus, enableFocus } = useFocusManager();

  const myFocus = (id: string): void => {
    focus(id);
    ctx.activeId = id;
  };

  return {
    activeId: ctx.activeId,
    focus: myFocus,
    enableFocus,
  };
}
