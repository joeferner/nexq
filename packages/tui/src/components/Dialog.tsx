import { Box, DOMElement, measureElement, Text } from "ink";
import React from "react";
import { Dimensions, useStdoutDimensions } from "../utils/useStdoutDimensions.js";

export interface DialogProps {
  borderColor: string;
  children: React.ReactNode | React.ReactNode[];
}

interface _DialogProps extends DialogProps {
  stdoutDimensions: Dimensions;
}

interface DialogState {
  top: number;
  left: number;
  width: number;
  height: number;
}

class _Dialog extends React.Component<_DialogProps, DialogState> {
  private ref = React.createRef<DOMElement>();

  public constructor(props: _DialogProps) {
    super(props);
    this.state = {
      top: 0,
      left: 0,
      width: 0,
      height: 0,
    };
  }

  public override componentDidMount(): void {
    this.resize();
  }

  public override componentDidUpdate(): void {
    this.resize();
  }

  private resize(): void {
    setTimeout(() => {
      if (this.ref.current) {
        const { stdoutDimensions } = this.props;
        const m = measureElement(this.ref.current);
        this.setState({
          left: (stdoutDimensions.columns - m.width) / 2,
          top: (stdoutDimensions.rows - m.height) / 2,
          width: m.width,
          height: m.height,
        });
      }
    });
  }

  public override render(): React.ReactNode {
    const { borderColor, children } = this.props;
    const { top, left } = this.state;

    const backgroundText = this.createBackgroundText();

    return (
      <Box
        ref={this.ref}
        position="absolute"
        marginTop={top}
        marginLeft={left}
        borderStyle="single"
        borderColor={borderColor}
      >
        <Box position="relative">
          <Box position="absolute">
            <Text>{backgroundText}</Text>
          </Box>
          <Box>{children}</Box>
        </Box>
      </Box>
    );
  }

  private createBackgroundText(): string {
    let results = "";
    for (let row = 0; row < this.state.height - 2; row++) {
      results += " ".repeat(this.state.width - 2);
      results += "\n";
    }
    return results;
  }
}

export function Dialog(props: DialogProps): React.ReactNode {
  const { columns, rows } = useStdoutDimensions();

  return <_Dialog {...props} stdoutDimensions={{ columns, rows }} />;
}
