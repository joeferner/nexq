import React, { createContext, useEffect, useState } from "react";
import { Api, GetInfoResponse, GetQueueResponse } from "./client/NexqClientApi.js";

export interface State {
    tuiVersion: string;
    info: GetInfoResponse | null;
    queues: GetQueueResponse[] | null;
    loadQueues: () => Promise<void>;
    selectedQueue: string | null;
    setSelectedQueue: (selectedQueue: string | null) => void;
    purgeQueue: (queueName: string) => Promise<void>;
}

export const StateContext = createContext<State>({
    tuiVersion: '???',
    info: null,
    queues: null,
    loadQueues: async () => { },
    selectedQueue: null,
    setSelectedQueue: () => { },
    purgeQueue: async () => { }
});

export function StateProvider(props: { api: Api<unknown>, tuiVersion: string, children: React.ReactNode | React.ReactNode[] }): React.ReactNode {
    const { children, api, tuiVersion } = props;

    const [info, setInfo] = useState<GetInfoResponse | null>(null);
    const [queues, setQueues] = useState<GetQueueResponse[] | null>(null);
    const [selectedQueue, setSelectedQueue] = useState<string | null>(null);

    useEffect(() => {
        const load = async (): Promise<void> => {
            const newInfo = await api.api.getInfo();
            setInfo(newInfo.data);
        }

        void load();
    }, []);

    const purgeQueue = async (queueName: string): Promise<void> => {
        await api.api.purgeQueue(queueName);
    }

    const loadQueues = async (): Promise<void> => {
        const queues = await api.api.getQueues();
        setQueues(queues.data.queues);
    }

    return (<StateContext.Provider value={{ tuiVersion, queues, info, selectedQueue, setSelectedQueue, purgeQueue, loadQueues }}>{children}</StateContext.Provider>);
}
