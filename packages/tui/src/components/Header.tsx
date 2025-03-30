import { Box, Text } from "ink";
import React, { ReactNode } from "react";
import { ApiContext } from "../ApiContext.js";

const NAME_COLOR = "#fca321";
const LOGO = `     __            ____ 
  /\\ \\ \\_____  __ /___ \\
 /  \\/ / _ \\ \\/ ///  / /
/ /\\  /  __/>  </ \\_/ / 
\\_\\ \\/ \\___/_/\\_\\___,_\\`;

export function Header(options: { tuiVersion: string }): ReactNode {
  const [nexqVersion, setNexqVersion] = React.useState("???");
  const api = React.useContext(ApiContext);

  React.useEffect(() => {
    const load = async (): Promise<void> => {
      if (!api) {
        return;
      }
      const info = await api.api.getInfo();
      setNexqVersion(info.data.version);
    };
    void load();
  }, [api]);

  return (
    <Box justifyContent="space-between">
      <Box flexDirection="column" justifyContent="flex-end">
        <Box>
          <Text color={NAME_COLOR}>TUI Ver: </Text>
          <Text color="white">v{options.tuiVersion}</Text>
        </Box>
        <Box>
          <Text color={NAME_COLOR}>NexQ Ver: </Text>
          <Text color="white">v{nexqVersion}</Text>
        </Box>
      </Box>
      <Box>
        <Text color={NAME_COLOR}>{LOGO}</Text>
      </Box>
    </Box>
  );
}
