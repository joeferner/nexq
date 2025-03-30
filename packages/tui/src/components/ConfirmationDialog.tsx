import { Box, Text } from "ink";
import React from "react";
import { MAIN_BORDER_COLOR } from "../styles.js";
import { Input } from "../utils/Input.js";
import { Dialog } from "./Dialog.js";

export const CONFIRMATION_DIALOG_ID = "ConfirmationDialog";

export interface ConfirmationDialogProps {
  message: string;
  options: string[];
  defaultOption: string;
  input: Input | null;
  onConfirm: (result: string | null) => unknown;
}

interface ConfirmationDialogState {
  selectedOption: string | null;
}

export class ConfirmationDialog extends React.Component<ConfirmationDialogProps, ConfirmationDialogState> {
  public constructor(props: ConfirmationDialogProps) {
    super(props);
    this.state = {
      selectedOption: null,
    };
  }

  public override componentDidMount(): void {
    this.setState({
      selectedOption: this.props.defaultOption,
    });
  }

  public override componentDidUpdate(
    prevProps: Readonly<ConfirmationDialogProps>,
    _prevState: Readonly<ConfirmationDialogState>
  ): void {
    const { input } = this.props;
    if (input && input?.t !== prevProps.input?.t) {
      void this.processInput(input);
    }
  }

  private async processInput(input: Input): Promise<void> {
    const { onConfirm } = this.props;
    const { selectedOption } = this.state;

    if (input.key.leftArrow) {
      this.updateConfirmationDialogSelectedOption(-1);
    } else if (input.key.rightArrow || input.key.tab) {
      this.updateConfirmationDialogSelectedOption(1);
    } else if (input.key.escape) {
      onConfirm(null);
    } else if (input.key.return) {
      onConfirm(selectedOption);
    }
  }

  private updateConfirmationDialogSelectedOption(dir: number): void {
    const { options } = this.props;
    const { selectedOption } = this.state;
    const currentIndex = selectedOption ? options.indexOf(selectedOption) : 0;
    let newIndex;
    if (currentIndex < -1) {
      newIndex = 0;
    } else {
      newIndex = (currentIndex + dir) % options.length;
    }
    this.setState({
      selectedOption: options[newIndex],
    });
  }

  public override render(): React.ReactNode {
    const { selectedOption } = this.state;
    const { message, options } = this.props;

    return (
      <Dialog borderColor={MAIN_BORDER_COLOR}>
        <Box paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1} flexDirection="column">
          <Text>{message}</Text>
          <Box gap={1} justifyContent="center" marginTop={1}>
            {options.map((o) => {
              const inverse = o === selectedOption;
              return <Text inverse={inverse}> {o} </Text>;
            })}
          </Box>
        </Box>
      </Dialog>
    );
  }
}
