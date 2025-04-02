import { Box, Text } from "ink";
import React, { ReactNode } from "react";
import { ApiContext, NexqClientApi } from "../ApiContext.js";
import { HOTKEY_COLOR, HOTKEY_NAME_COLOR } from "../styles.js";

const NAME_COLOR = "#fca321";
const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export interface HotKey {
  name: string;
  shortcut: string;
}

export interface HeaderProps {
  tuiVersion: string;
  hotkeys: HotKey[];
}

interface _HeaderProps extends HeaderProps {
  api: NexqClientApi;
}

interface HeaderState {
  nexqVersion: string;
}

class _Header extends React.Component<_HeaderProps, HeaderState> {
  public constructor(props: _HeaderProps) {
    super(props);
    this.state = {
      nexqVersion: '???'
    }
  }

  public override componentDidMount(): void {
    void this.load();
  }

  private async load(): Promise<void> {
    const info = await this.props.api.api.getInfo();
    this.setState({
      nexqVersion: info.data.version
    });
  }

  public override render(): ReactNode {
    const { tuiVersion, hotkeys } = this.props;
    const { nexqVersion } = this.state;

    return (
      <Box justifyContent="space-between">
        <Box flexDirection="column" justifyContent="flex-end">
          <Box>
            <Text color={NAME_COLOR}>TUI Ver: </Text>
            <Text color="white">v{tuiVersion}</Text>
          </Box>
          <Box>
            <Text color={NAME_COLOR}>NexQ Ver: </Text>
            <Text color="white">v{nexqVersion}</Text>
          </Box>
        </Box>
        <Box>
          {hotkeys.map(hotkey => {
            return (<Box>
              <Text color={HOTKEY_COLOR}>{`<${hotkey.shortcut}>`}</Text><Text color={HOTKEY_NAME_COLOR}> {hotkey.name}</Text>
            </Box>);
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
  const api = React.useContext(ApiContext);
  if (api === null) {
    return (<></>);
  }
  return (<_Header {...props} api={api} />);
}
