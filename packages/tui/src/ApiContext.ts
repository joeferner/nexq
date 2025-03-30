import { createContext } from "react";
import { Api } from "./client/NexqClientApi.js";

export type NexqClientApi = Api<unknown>;
export const ApiContext = createContext<NexqClientApi | null>(null);
