import React, { createContext } from "react";
import { Input } from "../utils/Input.js";
import { useNexqFocusManager } from "../utils/useNexqFocusManager.js";
import { CONFIRMATION_DIALOG_ID, ConfirmationDialog } from "./ConfirmationDialog.js";
import { ERROR_DIALOG_ID, ErrorDialog } from "./ErrorDialog.js";

export interface ShowConfirmationDialogOptions {
  message: string;
  options: string[];
  defaultOption: string;
}

export interface ShowErrorDialogOptions {
  message: string;
}

export interface DialogService {
  showConfirmationDialog: (options: ShowConfirmationDialogOptions) => Promise<string | null>;
  showErrorDialog: (options: ShowErrorDialogOptions) => Promise<void>;
}

export const DialogContext = createContext<DialogService>({
  showConfirmationDialog: async () => {
    return null;
  },
  showErrorDialog: async () => {},
});

interface DialogsProps {
  input: Input | null;
}

interface _DialogsProps extends DialogsProps {
  modalContext: DialogService;
  activeId: string | null;
  focus: (id: string) => void;
}

interface DialogsState {
  lastFocusId: string | null;
  confirmationDialogOptions?: ShowConfirmationDialogOptions;
  confirmationDialogResolve?: (result: string | null) => void;
  errorDialogOptions?: ShowErrorDialogOptions;
  errorDialogResolve?: () => void;
}

class _Dialogs extends React.Component<_DialogsProps, DialogsState> {
  public constructor(props: _DialogsProps) {
    super(props);
    this.state = {
      lastFocusId: null,
    };
  }

  public override componentDidMount(): void {
    const { modalContext } = this.props;
    modalContext.showConfirmationDialog = this.showConfirmationDialog.bind(this);
    modalContext.showErrorDialog = this.showErrorDialog.bind(this);
  }

  private showConfirmationDialog(options: ShowConfirmationDialogOptions): Promise<string | null> {
    const { activeId, focus } = this.props;

    this.setState({
      lastFocusId: activeId,
      confirmationDialogOptions: options,
    });
    focus(CONFIRMATION_DIALOG_ID);

    return new Promise<string | null>((resolve) => {
      this.setState({
        confirmationDialogResolve: resolve,
      });
    });
  }

  private handleConfirmationDialogConfirm(result: string | null): void {
    this.state.confirmationDialogResolve?.(result);
    if (this.state.lastFocusId) {
      this.props.focus(this.state.lastFocusId);
    }
    this.setState({
      confirmationDialogOptions: undefined,
      confirmationDialogResolve: undefined,
      lastFocusId: null,
    });
  }

  private showErrorDialog(options: ShowErrorDialogOptions): Promise<void> {
    const { activeId, focus } = this.props;

    this.setState({
      lastFocusId: activeId,
      errorDialogOptions: options,
    });
    focus(ERROR_DIALOG_ID);

    return new Promise<void>((resolve) => {
      this.setState({
        errorDialogResolve: resolve,
      });
    });
  }

  private handleErrorDialogConfirm(): void {
    this.state.errorDialogResolve?.();
    if (this.state.lastFocusId) {
      this.props.focus(this.state.lastFocusId);
    }
    this.setState({
      errorDialogOptions: undefined,
      errorDialogResolve: undefined,
      lastFocusId: null,
    });
  }

  public override render(): React.ReactNode {
    const { input } = this.props;
    const { errorDialogOptions, confirmationDialogOptions } = this.state;

    if (errorDialogOptions) {
      return <ErrorDialog {...errorDialogOptions} input={input} onConfirm={() => this.handleErrorDialogConfirm()} />;
    } else if (confirmationDialogOptions) {
      return (
        <ConfirmationDialog
          {...confirmationDialogOptions}
          input={input}
          onConfirm={(result) => this.handleConfirmationDialogConfirm(result)}
        />
      );
    } else {
      return <></>;
    }
  }
}

export function Dialogs(props: DialogsProps): React.ReactNode {
  const modalContext = React.useContext(DialogContext);
  const { activeId, focus } = useNexqFocusManager();

  return <_Dialogs {...props} modalContext={modalContext} activeId={activeId} focus={focus} />;
}
