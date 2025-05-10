# Message Deduplication

When sending a message, you can pass an optional deduplication id. NexQ will
not allow messages which have not been received with the same deduplication
id to be added to the queue. Once a message has been received even if the
message is errored, timed out, etc will not be considered for deduplication.
