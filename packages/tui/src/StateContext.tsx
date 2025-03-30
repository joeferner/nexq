import { Text } from "ink";
import React, { createContext, useEffect, useState } from "react";
import { Api, GetInfoResponse, GetQueueResponse } from "./client/NexqClientApi.js";
import { QUEUES_DEFAULT_STATE } from "./components/Queues.js";
import { TableViewState } from "./components/TableView.js";

export interface State {
  tuiVersion: string;
  info: GetInfoResponse | null;
  loadQueues: () => Promise<GetQueueResponse[]>;
  queuesTableViewState: TableViewState<GetQueueResponse>;
  setQueuesTableViewState: (state: TableViewState<GetQueueResponse>) => void;
  purgeQueue: (queueName: string) => Promise<void>;
  deleteQueue: (queueName: string) => Promise<void>;
  pauseQueue: (queueName: string) => Promise<void>;
  resumeQueue: (queueName: string) => Promise<void>;
  status: React.ReactNode;
  setStatus: (status: React.ReactNode) => void;
}

export const StateContext = createContext<State>({
  tuiVersion: "???",
  info: null,
  loadQueues: async () => {
    return [];
  },
  queuesTableViewState: QUEUES_DEFAULT_STATE,
  setQueuesTableViewState: () => {},
  purgeQueue: async () => {},
  deleteQueue: async () => {},
  pauseQueue: async () => {},
  resumeQueue: async () => {},
  status: <Text></Text>,
  setStatus: () => {},
});

interface Status {
  status: React.ReactNode;
  timeout: NodeJS.Timeout | null;
}

export function StateProvider(props: {
  api: Api<unknown>;
  tuiVersion: string;
  children: React.ReactNode | React.ReactNode[];
}): React.ReactNode {
  const { children, api, tuiVersion } = props;

  const [info, setInfo] = useState<GetInfoResponse | null>(null);
  const [queuesTableViewState, setQueuesTableViewState] =
    useState<TableViewState<GetQueueResponse>>(QUEUES_DEFAULT_STATE);
  const [_status, _setStatus] = useState<Status>({ status: <Text></Text>, timeout: null });

  useEffect(() => {
    const load = async (): Promise<void> => {
      const newInfo = await api.api.getInfo();
      setInfo(newInfo.data);
    };

    void load();
  }, []);

  const purgeQueue = async (queueName: string): Promise<void> => {
    await api.api.purgeQueue(queueName);
  };

  const deleteQueue = async (queueName: string): Promise<void> => {
    await api.api.deleteQueue(queueName);
  };

  const pauseQueue = async (queueName: string): Promise<void> => {
    await api.api.pauseQueue(queueName);
  };

  const resumeQueue = async (queueName: string): Promise<void> => {
    await api.api.resumeQueue(queueName);
  };

  const loadQueues = async (): Promise<GetQueueResponse[]> => {
    const queues = await api.api.getQueues();
    return queues.data.queues;
  };

  const setStatus = (status: React.ReactNode): void => {
    if (_status.timeout) {
      clearTimeout(_status.timeout);
    }

    const timeout = setTimeout(() => {
      _setStatus({
        status: <Text></Text>,
        timeout: null,
      });
    }, 1000);

    _setStatus({
      status,
      timeout,
    });
  };

  return (
    <StateContext.Provider
      value={{
        tuiVersion,
        info,
        queuesTableViewState,
        setQueuesTableViewState,
        purgeQueue,
        deleteQueue,
        loadQueues,
        pauseQueue,
        resumeQueue,
        status: _status.status,
        setStatus,
      }}
    >
      {children}
    </StateContext.Provider>
  );
}
