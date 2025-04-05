import React, { createContext, useEffect, useState } from "react";
import { Api, GetInfoResponse, GetQueueResponse } from "./client/NexqClientApi.js";
import { Text } from "ink";

export interface State {
  tuiVersion: string;
  info: GetInfoResponse | null;
  queues: GetQueueResponse[] | null;
  loadQueues: () => Promise<void>;
  selectedQueue: string | null;
  setSelectedQueue: (selectedQueue: string | null) => void;
  purgeQueue: (queueName: string) => Promise<void>;
  status: React.ReactNode;
  setStatus: (status: React.ReactNode) => void;
}

export const StateContext = createContext<State>({
  tuiVersion: "???",
  info: null,
  queues: null,
  loadQueues: async () => { },
  selectedQueue: null,
  setSelectedQueue: () => { },
  purgeQueue: async () => { },
  status: (<Text></Text>),
  setStatus: () => { }
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
  const [queues, setQueues] = useState<GetQueueResponse[] | null>(null);
  const [selectedQueue, setSelectedQueue] = useState<string | null>(null);
  const [_status, _setStatus] = useState<Status>({ status: (<Text></Text>), timeout: null });

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

  const loadQueues = async (): Promise<void> => {
    const queues = await api.api.getQueues();
    setQueues(queues.data.queues);
  };

  const setStatus = (status: React.ReactNode): void => {
    if (_status.timeout) {
      clearTimeout(_status.timeout);
    }

    const timeout = setTimeout(() => {
      _setStatus({
        status: (<Text></Text>),
        timeout: null
      })
    }, 1000);

    _setStatus({
      status,
      timeout
    });
  }

  return (
    <StateContext.Provider
      value={{ tuiVersion, queues, info, selectedQueue, setSelectedQueue, purgeQueue, loadQueues, status: _status.status, setStatus }}
    >
      {children}
    </StateContext.Provider>
  );
}
