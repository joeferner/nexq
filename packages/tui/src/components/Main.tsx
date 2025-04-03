import { Box, useApp, useInput } from "ink";
import React, { ReactNode, useEffect, useState } from "react";
import { Input } from "../utils/Input.js";
import { useNexqFocusManager } from "../utils/useNexqFocusManager.js";
import { Header } from "./Header.js";
import { Dialogs } from "./Dialogs.js";
import { QUEUE_HOTKEYS, Queues, QUEUES_ID } from "./Queues.js";

export interface MainProps {
  tuiVersion: string;
}

interface _MainProps extends MainProps {
  exit: () => void;
  input: Input | null;
}

class _Main extends React.Component<_MainProps> {
  public override componentDidUpdate(prevProps: Readonly<_MainProps>, _prevState: Readonly<unknown>): void {
    if (this.props.input && this.props.input?.t !== prevProps.input?.t) {
      this.processInput(this.props.input);
    }
  }

  private processInput(_input: Input): void {
  }

  public override render(): ReactNode {
    const { input, tuiVersion } = this.props;

    return (
      <Box flexDirection="column" height="100%">
        <Header tuiVersion={tuiVersion} hotkeys={QUEUE_HOTKEYS} />
        <Queues input={input} />
        <Dialogs input={input} />
      </Box>
    );
  }
}

export function Main(props: MainProps): ReactNode {
  const { exit } = useApp();
  const { focus, enableFocus } = useNexqFocusManager();
  const [input, setInput] = useState<Input | null>(null);
  useInput((text, key) => {
    setInput({ t: new Date(), text, key });
  });

  useEffect(() => {
    enableFocus();
    focus(QUEUES_ID);
  }, []);

  return <_Main {...props} exit={exit} input={input} />;
}
