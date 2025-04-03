import { useFocus } from "ink";
import * as R from "radash";
import React, { ReactNode } from "react";
import { ApiContext, NexqClientApi } from "../ApiContext.js";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { getErrorMessage } from "../utils/error.js";
import { Input, isInputMatch } from "../utils/Input.js";
import { DialogContext, DialogService } from "./Dialogs.js";
import { HotKey } from "./Header.js";
import { SortDirection, TableView, TableViewColumn } from "./TableView.js";
import { logToFile } from "../utils/log.js";

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
    valueFn: (row) => row.numberOfMessage,
    sortRows: (rows, direction) => R.sort(rows, (row) => row.numberOfMessage, direction === SortDirection.Descending),
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
  api: NexqClientApi;
  dialogService: DialogService;
  isFocused: boolean;
}

interface QueuesState {
  queues: GetQueueResponse[];
}

export class _Queues extends React.Component<_QueuesProps, QueuesState> {
  private loadTimeout?: NodeJS.Timeout;

  public constructor(props: _QueuesProps) {
    super(props);
    this.state = {
      queues: [],
    };
  }

  public override componentDidMount(): void {
    void this.load();
  }

  public override componentDidUpdate(
    prevProps: Readonly<_QueuesProps>,
    _prevState: Readonly<QueuesState>
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
    const { dialogService, api } = this.props;

    const queueName = 'TODO';
    const result = await dialogService.showConfirmationDialog({
      message: `Are you sure you want to purge "${queueName}"?`,
      options: ["Cancel", "Purge"],
      defaultOption: "Cancel"
    });
    if (result === 'Purge') {
      try {
        await api.api.purgeQueue(queueName);
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
    const { api } = this.props;

    if (this.loadTimeout) {
      clearTimeout(this.loadTimeout);
    }

    try {
      const resp = await api.api.getQueues();
      for (let i = 0; i < 100; i++) {
        resp.data.queues.push({
          name: `queue${i}`,
          numberOfMessage: i * 100,
          numberOfMessagesVisible: i * 100,
          numberOfMessagesNotVisible: i * 100,
          numberOfMessagesDelayed: i * 100,
        } as GetQueueResponse);
      }
      this.setState({
        queues: resp.data.queues,
      });
    } finally {
      this.loadTimeout = setTimeout(() => {
        void this.load();
      }, 5000);
    }
  }

  public override render(): ReactNode {
    const { input } = this.props;
    const { queues } = this.state;

    return <TableView id={QUEUES_ID} input={input} columns={COLUMNS} rows={queues} />;
  }
}

export function Queues(props: QueuesProps): ReactNode {
  const api = React.useContext(ApiContext);
  const dialogService = React.useContext(DialogContext);
  const { isFocused } = useFocus({ id: QUEUES_ID });

  if (!api) {
    return <></>;
  }
  return <_Queues {...props} api={api} dialogService={dialogService} isFocused={isFocused} />;
}
