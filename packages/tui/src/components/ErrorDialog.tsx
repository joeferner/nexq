import { Box, Text } from "ink";
import React from "react";
import { MAIN_BORDER_COLOR } from "../styles.js";
import { Input } from "../utils/Input.js";
import { Dialog } from "./Dialog.js";

export const ERROR_DIALOG_ID = "ErrorDialog";

export interface ErrorDialogProps {
  message: string;
  input: Input | null;
  onConfirm: () => unknown;
}

export class ErrorDialog extends React.Component<ErrorDialogProps> {
  public override componentDidUpdate(prevProps: Readonly<ErrorDialogProps>, _prevState: Readonly<unknown>): void {
    const { input } = this.props;
    if (input && input?.t !== prevProps.input?.t) {
      void this.processInput(input);
    }
  }

  private async processInput(input: Input): Promise<void> {
    const { onConfirm } = this.props;

    if (input.key.escape || input.key.return) {
      onConfirm();
    }
  }

  public override render(): React.ReactNode {
    const { message } = this.props;

    return (
      <Dialog borderColor={MAIN_BORDER_COLOR}>
        <Box paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1} flexDirection="column">
          <Text>{message}</Text>
        </Box>
      </Dialog>
    );
  }
}
