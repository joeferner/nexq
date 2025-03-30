import { ReactNode, useEffect } from "react";

const ENTER_ALT_SCREEN_COMMAND = "\x1b[?1049h";
const CLEAR_SCREEN_COMMAND = "\x1B[2J\x1B[0f";
const LEAVE_ALT_SCREEN_COMMAND = "\x1b[?1049l";

export function exitFullScreen(): void {
  process.stdout.write(LEAVE_ALT_SCREEN_COMMAND);
}

export function FullScreen(options: { children: ReactNode | ReactNode[] }): ReactNode | ReactNode[] {
  useEffect(() => {
    return exitFullScreen;
  }, []);

  process.stdout.write(ENTER_ALT_SCREEN_COMMAND);
  process.stdout.write(CLEAR_SCREEN_COMMAND);
  return options.children;
}
