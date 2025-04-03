import { Box, DOMElement, measureElement, Text } from "ink";
import React, { createContext } from "react";
import { MAIN_BORDER_COLOR } from "../styles.js";
import { Input } from "../utils/Input.js";
import { useNexqFocusManager } from "../utils/useNexqFocusManager.js";
import { Dimensions, useStdoutDimensions } from "../utils/useStdoutDimensions.js";

export const CONFIRMATION_DIALOG_ID = 'ConfirmationDialog';

export interface ShowConfirmationDialogOptions {
    message: string;
    options: string[];
    selectedOption: string;
}

export interface ShowErrorDialogOptions {
    message: string;
}

export interface DialogService {
    showConfirmationDialog: (options: ShowConfirmationDialogOptions) => Promise<string | null>;
    showErrorDialog: (options: ShowErrorDialogOptions) => Promise<void>;
}

export const DialogContext = createContext<DialogService>({
    showConfirmationDialog: async () => { return null; },
    showErrorDialog: async () => { }
});

interface DialogProps {
    input: Input | null;
}

interface _DialogProps extends DialogProps {
    modalContext: DialogService;
    stdoutDimensions: Dimensions;
    activeId: string | null;
    focus: (id: string) => void;
}

interface DialogState {
    top: number;
    left: number;
    borderColor: string;
    lastFocusId: string | null;
    confirmationDialogOptions?: ShowConfirmationDialogOptions;
    confirmationDialogResolve?: (result: string | null) => void;
    confirmationDialogReject?: (err: unknown) => void;
}

class _Dialog extends React.Component<_DialogProps, DialogState> {
    private ref = React.createRef<DOMElement>();

    public constructor(props: _DialogProps) {
        super(props);
        this.state = {
            top: 0,
            left: 0,
            borderColor: MAIN_BORDER_COLOR,
            lastFocusId: null
        };
    }

    public override componentDidMount(): void {
        const { modalContext } = this.props;
        modalContext.showConfirmationDialog = this.showConfirmationDialog.bind(this);
    }

    private showConfirmationDialog(options: ShowConfirmationDialogOptions): Promise<string | null> {
        const { activeId, focus } = this.props;

        this.setState({
            lastFocusId: activeId,
            borderColor: MAIN_BORDER_COLOR,
            confirmationDialogOptions: options
        });
        focus(CONFIRMATION_DIALOG_ID);
        setTimeout(() => {
            if (this.ref.current) {
                const { stdoutDimensions } = this.props;
                const m = measureElement(this.ref.current);
                this.setState({
                    left: (stdoutDimensions.columns - m.width) / 2,
                    top: (stdoutDimensions.rows - m.height) / 2
                })
            }
        });
        return new Promise<string | null>((resolve, reject) => {
            this.setState({
                confirmationDialogResolve: resolve,
                confirmationDialogReject: reject
            });
        });
    }

    public override componentDidUpdate(
        prevProps: Readonly<_DialogProps>,
        _prevState: Readonly<DialogState>
    ): void {
        const { activeId, input } = this.props;
        if (input && input?.t !== prevProps.input?.t) {
            if (activeId === CONFIRMATION_DIALOG_ID) {
                void this.processConfirmationInput(input);
            }
        }
    }

    private async processConfirmationInput(input: Input): Promise<void> {
        if (!this.state.confirmationDialogOptions) {
            throw new Error('invalid state, confirmationDialogOptions is not set');
        }

        if (input.key.leftArrow) {
            this.updateConfirmationDialogSelectedOption(-1);
        } else if (input.key.rightArrow || input.key.tab) {
            this.updateConfirmationDialogSelectedOption(1);
        } else if (input.key.escape) {
            this.state.confirmationDialogResolve?.(null);
            this.clearConfirmationDialog();
        } else if (input.key.return) {
            this.state.confirmationDialogResolve?.(this.state.confirmationDialogOptions.selectedOption ?? '');
            this.clearConfirmationDialog();
        }
    }

    private updateConfirmationDialogSelectedOption(dir: number): void {
        if (!this.state.confirmationDialogOptions) {
            throw new Error('invalid state, confirmationDialogOptions is not set');
        }
        const { options, selectedOption } = this.state.confirmationDialogOptions;
        const currentIndex = options.indexOf(selectedOption);
        let newIndex;
        if (currentIndex < -1) {
            newIndex = 0;
        } else {
            newIndex = (currentIndex + dir) % options.length;
        }
        this.setState({
            confirmationDialogOptions: {
                ...this.state.confirmationDialogOptions,
                selectedOption: options[newIndex]
            }
        });
    }

    private clearConfirmationDialog(): void {
        this.setState({
            confirmationDialogOptions: undefined,
            confirmationDialogReject: undefined,
            confirmationDialogResolve: undefined
        });
    }

    public override render(): React.ReactNode {
        const { confirmationDialogOptions } = this.state;

        if (confirmationDialogOptions) {
            return this.renderConfirmationDialogOptions(confirmationDialogOptions);
        } else {
            return (<></>);
        }
    }

    private renderConfirmationDialogOptions(options: ShowConfirmationDialogOptions): React.ReactNode {
        const { top, left, borderColor } = this.state;

        return (<Box ref={this.ref} position="absolute" marginTop={top} marginLeft={left} borderStyle="single" borderColor={borderColor}>
            <Box paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1} flexDirection="column">
                <Text>{options.message}</Text>
                <Box gap={1} justifyContent="center" marginTop={1}>
                    {options.options.map(o => {
                        const inverse = o === options.selectedOption;
                        return (<Text inverse={inverse}> {o} </Text>);
                    })}
                </Box>
            </Box>
        </Box>);
    }
}

export function Dialog(props: DialogProps): React.ReactNode {
    const modalContext = React.useContext(DialogContext);
    const { activeId, focus } = useNexqFocusManager();
    const { columns, rows } = useStdoutDimensions();

    return (<_Dialog {...props} modalContext={modalContext} stdoutDimensions={{ columns, rows }} activeId={activeId} focus={focus} />);
}
