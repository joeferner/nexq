import { Store } from "@nexq/core";
import { IocContainer, IocContainerFactory, ServiceIdentifier } from "tsoa";
import { ApiV1QueueController } from "./controllers/ApiV1QueueController.js";
import { ApiV1TopicController } from "./controllers/ApiV1TopicController.js";

let _store: Store | undefined;

function _iocContainer(_request: unknown): IocContainer {
  return {
    get: <T>(controller: ServiceIdentifier<T>): T | Promise<T> => {
      if (_store == undefined) {
        throw new Error("store not initialized");
      }
      if (controller === ApiV1QueueController) {
        return new ApiV1QueueController(_store) as T;
      } else if (controller === ApiV1TopicController) {
        return new ApiV1TopicController(_store) as T;
      } else {
        throw new Error(`unhandled controller: ${String(controller)}`);
      }
    },
  };
}

_iocContainer.setStore = (store: Store): void => {
  _store = store;
};

export const iocContainer: IocContainerFactory & { setStore: (store: Store) => void } = _iocContainer;
