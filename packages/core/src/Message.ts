import { Buffer } from "node:buffer";

export interface CreateMessageOptions {
  id: string;
  receiptHandle: string;
  body: Buffer;
  sentTime: Date;
  priority: number;
  attributes: Record<string, string>;
}

export class Message {
  public readonly id: string;
  public readonly receiptHandle: string;
  public readonly body: Buffer;
  public readonly priority: number;
  public readonly attributes: Record<string, string>;

  /**
   * Time the message was originally sent
   */
  public readonly sentTime: Date;

  public constructor(options: CreateMessageOptions) {
    this.id = options.id;
    this.receiptHandle = options.receiptHandle;
    this.body = options.body;
    this.sentTime = options.sentTime;
    this.priority = options.priority;
    this.attributes = options.attributes;
  }

  public bodyAsString(encoding?: BufferEncoding): string {
    return this.body.toString(encoding);
  }
}
