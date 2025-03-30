import { Box, useApp, useInput } from "ink";
import React, { ReactNode, useContext, useEffect, useState } from "react";
import { StateContext } from "../StateContext.js";
import { Input } from "../utils/Input.js";
import { useNexqFocusManager } from "../utils/useNexqFocusManager.js";
import { Dialogs } from "./Dialogs.js";
import { Header } from "./Header.js";
import { QUEUE_HOTKEYS, Queues, QUEUES_ID } from "./Queues.js";

export interface MainProps {
  _placeholder?: unknown;
}

interface _MainProps extends MainProps {
  exit: () => void;
  input: Input | null;
  status: React.ReactNode;
}

class _Main extends React.Component<_MainProps> {
  public override componentDidUpdate(prevProps: Readonly<_MainProps>, _prevState: Readonly<unknown>): void {
    if (this.props.input && this.props.input?.t !== prevProps.input?.t) {
      this.processInput(this.props.input);
    }
  }

  private processInput(_input: Input): void {}

  public override render(): ReactNode {
    const { input, status } = this.props;

    return (
      <Box flexDirection="column" height="100%">
        <Header hotkeys={QUEUE_HOTKEYS} />
        <Queues input={input} />
        <Box justifyContent="center">{status}</Box>
        <Dialogs input={input} />
      </Box>
    );
  }
}

export function Main(props: MainProps): ReactNode {
  const { exit } = useApp();
  const { status } = useContext(StateContext);
  const { focus, enableFocus } = useNexqFocusManager();
  const [input, setInput] = useState<Input | null>(null);
  useInput((text, key) => {
    setInput({ t: new Date(), text, key });
  });

  useEffect(() => {
    enableFocus();
    focus(QUEUES_ID);
  }, []);

  return <_Main {...props} exit={exit} input={input} status={status} />;
}
