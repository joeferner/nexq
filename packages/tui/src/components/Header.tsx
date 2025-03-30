import { Box, Text } from "ink";
import React, { ReactNode } from "react";
import { StateContext } from "../StateContext.js";
import { GetInfoResponse } from "../client/NexqClientApi.js";
import { HOTKEY_COLOR, HOTKEY_NAME_COLOR } from "../styles.js";

const NAME_COLOR = "#fca321";
const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export interface HotKey {
  id: string;
  name: string;
  shortcut: string;
}

export interface HeaderProps {
  hotkeys: HotKey[];
}

interface _HeaderProps extends HeaderProps {
  tuiVersion: string;
  info: GetInfoResponse | null;
}

class _Header extends React.Component<_HeaderProps> {
  public override render(): ReactNode {
    const { info, hotkeys, tuiVersion } = this.props;

    return (
      <Box justifyContent="space-between">
        <Box flexDirection="column" justifyContent="flex-end">
          <Box>
            <Text color={NAME_COLOR}>TUI Ver: </Text>
            <Text color="white">v{tuiVersion}</Text>
          </Box>
          <Box>
            <Text color={NAME_COLOR}>NexQ Ver: </Text>
            <Text color="white">v{info?.version ?? "???"}</Text>
          </Box>
        </Box>
        <Box flexDirection="column">
          {hotkeys.map((hotkey) => {
            return (
              <Box>
                <Text color={HOTKEY_COLOR}>{`<${hotkey.shortcut}>`}</Text>
                <Text color={HOTKEY_NAME_COLOR}> {hotkey.name}</Text>
              </Box>
            );
          })}
        </Box>
        <Box>
          <Text color={NAME_COLOR}>{LOGO}</Text>
        </Box>
      </Box>
    );
  }
}

export function Header(props: HeaderProps): ReactNode {
  const state = React.useContext(StateContext);
  return <_Header {...props} info={state.info} tuiVersion={state.tuiVersion} />;
}
