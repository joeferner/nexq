import { useFocus } from "ink";
import * as R from "radash";
import React, { ReactNode } from "react";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { StateContext } from "../StateContext.js";
import { getErrorMessage } from "../utils/error.js";
import { Input, isInputMatch } from "../utils/Input.js";
import { DialogContext, DialogService } from "./Dialogs.js";
import { HotKey } from "./Header.js";
import { SortDirection, TableView, TableViewColumn } from "./TableView.js";

export const QUEUES_ID = "queues";

export const QUEUE_HOTKEYS: HotKey[] = [
  {
    id: 'purge',
    name: 'Purge',
    shortcut: 'ctrl-u'
  }
];

const COLUMNS: TableViewColumn<GetQueueResponse>[] = [
  {
    name: "NAME",
    sortKeyboardShortcut: "shift-n",
    valueFn: (row) => row.name,
    sortRows: (rows, direction) => R.alphabetical(rows, (row) => row.name, direction),
  },
  {
    name: "COUNT",
    sortKeyboardShortcut: "shift-c",
    valueFn: (row) => row.numberOfMessages,
    sortRows: (rows, direction) => R.sort(rows, (row) => row.numberOfMessages, direction === SortDirection.Descending),
  },
  {
    name: "VISIBLE",
    sortKeyboardShortcut: "shift-v",
    valueFn: (row) => row.numberOfMessagesVisible,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesVisible, direction === SortDirection.Descending),
  },
  {
    name: "NOT VISIBLE",
    sortKeyboardShortcut: "shift-o",
    valueFn: (row) => row.numberOfMessagesNotVisible,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesNotVisible, direction === SortDirection.Descending),
  },
  {
    name: "DELAYED",
    sortKeyboardShortcut: "shift-d",
    valueFn: (row) => row.numberOfMessagesDelayed,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesDelayed, direction === SortDirection.Descending),
  },
];

export interface QueuesProps {
  input: Input | null;
}

interface _QueuesProps extends QueuesProps {
  queues: GetQueueResponse[] | null;
  dialogService: DialogService;
  isFocused: boolean;
  purgeQueue: (queueName: string) => Promise<void>;
  loadQueues: () => Promise<void>;
}

export class _Queues extends React.Component<_QueuesProps> {
  private loadTimeout?: NodeJS.Timeout;

  public override componentDidMount(): void {
    void this.load();
  }

  public override componentDidUpdate(
    prevProps: Readonly<_QueuesProps>,
    _prevState: Readonly<unknown>
  ): void {
    const { isFocused, input } = this.props;
    if (isFocused && input && input?.t !== prevProps.input?.t) {
      void this.processInput(input);
    }
  }

  private async processInput(input: Input): Promise<void> {
    for (const hotkey of QUEUE_HOTKEYS) {
      if (isInputMatch(input, hotkey.shortcut)) {
        switch (hotkey.id) {
          case 'purge':
            await this.purgeSelectedQueue();
            break;
        }
      }
    }
  }

  private async purgeSelectedQueue(): Promise<void> {
    const { dialogService, purgeQueue } = this.props;

    const queueName = 'TODO';
    const result = await dialogService.showConfirmationDialog({
      message: `Are you sure you want to purge "${queueName}"?`,
      options: ["Cancel", "Purge"],
      defaultOption: "Cancel"
    });
    if (result === 'Purge') {
      try {
        await purgeQueue(queueName);
      } catch (err) {
        void dialogService.showErrorDialog({
          message: `Failed to purge "${queueName}".\n\n${getErrorMessage(err)}`
        });
      } finally {
        void this.load();
      }
    }
  }

  private async load(): Promise<void> {
    const { loadQueues } = this.props;

    if (this.loadTimeout) {
      clearTimeout(this.loadTimeout);
    }

    try {
      await loadQueues();
    } finally {
      this.loadTimeout = setTimeout(() => {
        void this.load();
      }, 5000);
    }
  }

  public override render(): ReactNode {
    const { input, queues } = this.props;

    return <TableView id={QUEUES_ID} input={input} columns={COLUMNS} rows={queues ?? []} />;
  }
}

export function Queues(props: QueuesProps): ReactNode {
  const state = React.useContext(StateContext);
  const dialogService = React.useContext(DialogContext);
  const { isFocused } = useFocus({ id: QUEUES_ID });

  return <_Queues {...props} queues={state.queues} purgeQueue={state.purgeQueue} loadQueues={state.loadQueues} dialogService={dialogService} isFocused={isFocused} />;
}
