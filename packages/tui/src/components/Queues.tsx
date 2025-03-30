import { Text, useFocus } from "ink";
import * as R from "radash";
import React, { ReactNode } from "react";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { StateContext } from "../StateContext.js";
import { getErrorMessage } from "../utils/error.js";
import { Input, isInputMatch } from "../utils/Input.js";
import { DialogContext, DialogService } from "./Dialogs.js";
import { HotKey } from "./Header.js";
import { SortDirection, TableView, TableViewColumn, TableViewState } from "./TableView.js";

export const QUEUES_ID = "queues";

export const QUEUE_HOTKEYS: HotKey[] = [
  {
    id: "purge",
    name: "Purge",
    shortcut: "ctrl-u",
  },
  {
    id: "delete",
    name: "Delete",
    shortcut: "ctrl-d",
  },
  {
    id: "pause",
    name: "Pause/Resume",
    shortcut: "ctrl-r",
  },
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
  {
    name: "STATUS",
    sortKeyboardShortcut: "shift-s",
    valueFn: (row) => (row.paused ? "P" : ""),
    sortRows: (rows, direction) => R.sort(rows, (row) => (row.paused ? 1 : 0), direction === SortDirection.Descending),
  },
];

export const QUEUES_DEFAULT_STATE: TableViewState<GetQueueResponse> = {
  offset: 0,
  selectedRowId: null,
  sortColumn: COLUMNS[0],
  sortColumnDirection: SortDirection.Ascending,
};

export interface QueuesProps {
  input: Input | null;
}

interface _QueuesProps extends QueuesProps {
  dialogService: DialogService;
  isFocused: boolean;
  tableViewState: TableViewState<GetQueueResponse>;
  setTableViewState: (state: TableViewState<GetQueueResponse>) => void;
  setStatus: (status: React.ReactNode) => void;
  purgeQueue: (queueName: string) => Promise<void>;
  deleteQueue: (queueName: string) => Promise<void>;
  pauseQueue: (queueName: string) => Promise<void>;
  resumeQueue: (queueName: string) => Promise<void>;
  loadQueues: () => Promise<GetQueueResponse[]>;
}

interface QueuesState {
  queues: (GetQueueResponse & { id: string })[];
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

  public override componentDidUpdate(prevProps: Readonly<_QueuesProps>, _prevState: Readonly<unknown>): void {
    const { isFocused, input } = this.props;
    if (isFocused && input && input?.t !== prevProps.input?.t) {
      void this.processInput(input);
    }

    if (
      this.props.tableViewState.sortColumn !== prevProps.tableViewState.sortColumn ||
      this.props.tableViewState.sortColumnDirection !== prevProps.tableViewState.sortColumnDirection
    ) {
      this.setState({
        queues: this.sortQueues(this.state.queues),
      });
    }
  }

  private sortQueues(queues: GetQueueResponse[]): (GetQueueResponse & { id: string })[] {
    const { sortColumn, sortColumnDirection } = this.props.tableViewState;

    queues = sortColumn.sortRows(queues, sortColumnDirection);
    return queues.map((q) => {
      return {
        id: q.name,
        ...q,
      };
    });
  }

  public override componentWillUnmount(): void {
    if (this.loadTimeout) {
      clearTimeout(this.loadTimeout);
    }
  }

  private async processInput(input: Input): Promise<void> {
    for (const hotkey of QUEUE_HOTKEYS) {
      if (isInputMatch(input, hotkey.shortcut)) {
        switch (hotkey.id) {
          case "purge":
            await this.purgeSelectedQueue();
            break;
          case "delete":
            await this.deleteSelectedQueue();
            break;
          case "pause":
            await this.pauseResumeSelectedQueue();
            break;
        }
      }
    }
  }

  private async pauseResumeSelectedQueue(): Promise<void> {
    const { dialogService, resumeQueue, pauseQueue, tableViewState, setStatus } = this.props;

    if (!tableViewState.selectedRowId) {
      void dialogService.showErrorDialog({
        message: `No selected queue`,
      });
      return;
    }

    const queueName = tableViewState.selectedRowId;
    const queue = this.state.queues.find((q) => q.name === queueName);
    if (!queue) {
      void dialogService.showErrorDialog({
        message: `Failed to get queue`,
      });
      return;
    }

    if (queue.paused) {
      try {
        await resumeQueue(queueName);
        setStatus(<Text>Queue "{queueName}" resumed!</Text>);
      } catch (err) {
        void dialogService.showErrorDialog({
          message: `Failed to resume "${queueName}".\n\n${getErrorMessage(err)}`,
        });
      } finally {
        void this.load();
      }
    } else {
      try {
        await pauseQueue(queueName);
        setStatus(<Text>Queue "{queueName}" paused!</Text>);
      } catch (err) {
        void dialogService.showErrorDialog({
          message: `Failed to pause "${queueName}".\n\n${getErrorMessage(err)}`,
        });
      } finally {
        void this.load();
      }
    }
  }

  private async purgeSelectedQueue(): Promise<void> {
    const { dialogService, purgeQueue, tableViewState, setStatus } = this.props;

    if (!tableViewState.selectedRowId) {
      void dialogService.showErrorDialog({
        message: `No selected queue`,
      });
      return;
    }

    const queueName = tableViewState.selectedRowId;
    const result = await dialogService.showConfirmationDialog({
      message: `Are you sure you want to purge "${queueName}"?`,
      options: ["Cancel", "Purge"],
      defaultOption: "Cancel",
    });
    if (result === "Purge") {
      try {
        await purgeQueue(queueName);
        setStatus(<Text>Queue "{queueName}" purged!</Text>);
      } catch (err) {
        void dialogService.showErrorDialog({
          message: `Failed to purge "${queueName}".\n\n${getErrorMessage(err)}`,
        });
      } finally {
        void this.load();
      }
    }
  }

  private async deleteSelectedQueue(): Promise<void> {
    const { dialogService, deleteQueue, tableViewState, setStatus } = this.props;

    if (!tableViewState.selectedRowId) {
      void dialogService.showErrorDialog({
        message: `No selected queue`,
      });
      return;
    }

    const queueName = tableViewState.selectedRowId;
    const result = await dialogService.showConfirmationDialog({
      message: `Are you sure you want to delete "${queueName}"?`,
      options: ["Cancel", "Delete"],
      defaultOption: "Cancel",
    });
    if (result === "Delete") {
      try {
        await deleteQueue(queueName);
        setStatus(<Text>Queue "{queueName}" deleted!</Text>);
      } catch (err) {
        void dialogService.showErrorDialog({
          message: `Failed to delete "${queueName}".\n\n${getErrorMessage(err)}`,
        });
      } finally {
        void this.load();
      }
    }
  }

  private async load(): Promise<void> {
    const { loadQueues, tableViewState, setTableViewState } = this.props;

    if (this.loadTimeout) {
      clearTimeout(this.loadTimeout);
    }

    try {
      const queues = await loadQueues();
      this.setState({
        queues: this.sortQueues(queues),
      });
      if (
        queues.length > 0 &&
        (tableViewState.selectedRowId === null || !queues.some((q) => q.name === tableViewState.selectedRowId))
      ) {
        setTableViewState({
          ...tableViewState,
          selectedRowId: queues[0].name,
        });
      }
    } finally {
      this.loadTimeout = setTimeout(() => {
        void this.load();
      }, 5000);
    }
  }

  public override render(): ReactNode {
    const { input, setTableViewState, tableViewState } = this.props;
    const { queues } = this.state;

    return (
      <TableView
        id={QUEUES_ID}
        input={input}
        columns={COLUMNS}
        rows={queues}
        state={tableViewState}
        stateChanged={setTableViewState}
      />
    );
  }
}

export function Queues(props: QueuesProps): ReactNode {
  const {
    purgeQueue,
    deleteQueue,
    loadQueues,
    pauseQueue,
    resumeQueue,
    queuesTableViewState,
    setQueuesTableViewState,
    setStatus,
  } = React.useContext(StateContext);
  const dialogService = React.useContext(DialogContext);
  const { isFocused } = useFocus({ id: QUEUES_ID });

  return (
    <_Queues
      {...props}
      purgeQueue={purgeQueue}
      deleteQueue={deleteQueue}
      loadQueues={loadQueues}
      pauseQueue={pauseQueue}
      resumeQueue={resumeQueue}
      dialogService={dialogService}
      isFocused={isFocused}
      tableViewState={queuesTableViewState}
      setTableViewState={setQueuesTableViewState}
      setStatus={setStatus}
    />
  );
}
