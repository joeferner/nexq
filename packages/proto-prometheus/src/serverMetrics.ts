import { Store } from "@nexq/core";
import { Response as ExpressResponse } from "express";

export async function serverMetrics(store: Store, res: ExpressResponse): Promise<void> {
  const text = await createMetrics(store);
  res.setHeader("Content-Type", "text/plain");
  res.send(text);
}

export async function createMetrics(store: Store): Promise<string> {
  const results: string[] = [];

  const mem = process.memoryUsage();
  results.push(`# TYPE node_heap_used gauge`);
  results.push(`# HELP node_heap_used Amount of heap memory used`);
  results.push(`node_heap_used ${mem.heapUsed}`);

  results.push(`# TYPE node_heap_total gauge`);
  results.push(`# HELP node_heap_total Amount of total heap memory`);
  results.push(`node_heap_total ${mem.heapTotal}`);

  const queueInfos = await store.getQueueInfos();
  if (queueInfos.length > 0) {
    results.push(`# TYPE nexq_queue_messages gauge`);
    results.push(`# HELP nexq_queue_messages Messages ready to be delivered`);
    for (const queueInfo of queueInfos) {
      results.push(`nexq_queue_messages{queue="${queueInfo.name}"} ${queueInfo.numberOfMessages}`);
    }

    results.push(`# TYPE nexq_queue_messages_delayed gauge`);
    results.push(`# HELP nexq_queue_messages_delayed Messages delayed`);
    for (const queueInfo of queueInfos) {
      results.push(`nexq_queue_messages_delayed{queue="${queueInfo.name}"} ${queueInfo.numberOfMessagesDelayed}`);
    }

    results.push(`# TYPE nexq_queue_messages_not_visible gauge`);
    results.push(`# HELP nexq_queue_messages_not_visible Messages currently being processed`);
    for (const queueInfo of queueInfos) {
      results.push(
        `nexq_queue_messages_not_visible{queue="${queueInfo.name}"} ${queueInfo.numberOfMessagesNotVisible}`
      );
    }
  }

  return results.join("\n");
}
