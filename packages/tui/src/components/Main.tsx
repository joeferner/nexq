import { Box, useApp, useFocusManager, useInput } from "ink";
import React, { ReactNode, useEffect, useState } from "react";
import { Input } from "../utils/Input.js";
import { Header } from "./Header.js";
import { Queues, QUEUES_ID } from "./Queues.js";

export interface MainProps {
  tuiVersion: string;
}

interface _MainProps extends MainProps {
  focus: (id: string) => void;
  exit: () => void;
  input: Input | null;
}

class _Main extends React.Component<_MainProps> {
  public override componentDidUpdate(prevProps: Readonly<_MainProps>, _prevState: Readonly<unknown>): void {
    if (this.props.input && this.props.input?.t !== prevProps.input?.t) {
      this.processInput(this.props.input);
    }
  }

  private processInput(input: Input): void {
    if (input.key.escape) {
      this.props.exit();
    }
  }

  public override render(): ReactNode {
    return (
      <Box flexDirection="column" height="100%">
        <Header tuiVersion={this.props.tuiVersion} />
        <Queues />
      </Box>
    );
  }
}

export function Main(props: MainProps): ReactNode {
  const { exit } = useApp();
  const { focus, enableFocus } = useFocusManager();
  const [input, setInput] = useState<Input | null>(null);
  useInput((text, key) => {
    setInput({ t: new Date(), text, key });
  });

  useEffect(() => {
    enableFocus();
    focus(QUEUES_ID);
  }, []);

  return <_Main {...props} exit={exit} focus={focus} input={input} />;
}
