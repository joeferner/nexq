import { DEFAULT_MAX_NUMBER_OF_MESSAGES } from "../Store.js";

export interface PeekMessagesOptions {
  maxNumberOfMessages?: number;
  includeNotVisible?: boolean;
  includeDelayed?: boolean;
}

export function toRequiredPeekMessagesOptions(options?: PeekMessagesOptions): Required<PeekMessagesOptions> {
  const peekMessagesOptions: Required<PeekMessagesOptions> = {
    maxNumberOfMessages: DEFAULT_MAX_NUMBER_OF_MESSAGES,
    includeDelayed: false,
    includeNotVisible: false,
    ...options,
  };
  if (peekMessagesOptions.maxNumberOfMessages === undefined) {
    peekMessagesOptions.maxNumberOfMessages = DEFAULT_MAX_NUMBER_OF_MESSAGES;
  }
  if (peekMessagesOptions.includeDelayed === undefined) {
    peekMessagesOptions.includeDelayed = false;
  }
  if (peekMessagesOptions.includeNotVisible === undefined) {
    peekMessagesOptions.includeNotVisible = false;
  }
  return peekMessagesOptions;
}
