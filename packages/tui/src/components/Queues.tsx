import { useFocus } from "ink";
import React, { ReactNode } from "react";
import { ApiContext, NexqClientApi } from "../ApiContext.js";
import { GetQueueResponse } from "../client/NexqClientApi.js";
import { SortDirection, TableView, TableViewColumn } from "./TableView.js";
import * as R from "radash";
import { HotKey } from "./Header.js";

export const QUEUES_ID = "queues";

export const QUEUE_HOT_KEYS: HotKey[] = [
  {
    name: 'Purge',
    shortcut: 'ctrl-p'
  }
];

const COLUMNS: TableViewColumn<GetQueueResponse>[] = [
  {
    name: "NAME",
    sortKeyboardShortcut: "shift+n",
    valueFn: (row) => row.name,
    sortRows: (rows, direction) => R.alphabetical(rows, (row) => row.name, direction),
  },
  {
    name: "COUNT",
    sortKeyboardShortcut: "shift+c",
    valueFn: (row) => row.numberOfMessage,
    sortRows: (rows, direction) => R.sort(rows, (row) => row.numberOfMessage, direction === SortDirection.Descending),
  },
  {
    name: "VISIBLE",
    sortKeyboardShortcut: "shift+v",
    valueFn: (row) => row.numberOfMessagesVisible,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesVisible, direction === SortDirection.Descending),
  },
  {
    name: "NOT VISIBLE",
    sortKeyboardShortcut: "shift+o",
    valueFn: (row) => row.numberOfMessagesNotVisible,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesNotVisible, direction === SortDirection.Descending),
  },
  {
    name: "DELAYED",
    sortKeyboardShortcut: "shift+d",
    valueFn: (row) => row.numberOfMessagesDelayed,
    sortRows: (rows, direction) =>
      R.sort(rows, (row) => row.numberOfMessagesDelayed, direction === SortDirection.Descending),
  },
];

export interface _QueuesProps {
  api: NexqClientApi;
  isFocused: boolean;
}

export interface QueuesState {
  queues: GetQueueResponse[];
}

export class _Queues extends React.Component<_QueuesProps, QueuesState> {
  public constructor(props: _QueuesProps) {
    super(props);
    this.state = {
      queues: [],
    };
  }

  public override componentDidMount(): void {
    void this.load();
  }

  private async load(): Promise<void> {
    const { api } = this.props;

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
  }

  public override render(): ReactNode {
    const { queues } = this.state;

    return <TableView id={QUEUES_ID} columns={COLUMNS} rows={queues} />;
  }
}

export function Queues(): ReactNode {
  const api = React.useContext(ApiContext);
  const { isFocused } = useFocus({ id: QUEUES_ID });

  if (!api) {
    return <></>;
  }
  return <_Queues api={api} isFocused={isFocused} />;
}
