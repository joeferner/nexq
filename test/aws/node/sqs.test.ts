import { afterAll, describe, expect, test, } from '@jest/globals';
import { SQSClient, ListQueuesCommand, CreateQueueCommand, GetQueueAttributesCommand, GetQueueUrlCommand, SendMessageCommand, PurgeQueueCommand, DeleteQueueCommand, ReceiveMessageCommand, ChangeMessageVisibilityCommand, DeleteMessageCommand } from "@aws-sdk/client-sqs";
import { deleteAllQueues, getQueueNumberOfMessages, sqsClientOpts } from './utils';

const messageTest1Body = "test1 !@#$$%^&*()_+:/";
const messageTest1Md5OfMessageBody = "0fa181988e7c9840758db6e0f3405f0c";

describe('sqs', () => {
    const sqsClient = new SQSClient(sqsClientOpts);
    let queue1Url = '';
    let deadLetterUrl = '';

    afterAll(async () => {
        await deleteAllQueues(sqsClient);
    });

    test('create dead letter queue', async () => {
        const response = await sqsClient.send(new CreateQueueCommand({
            QueueName: 'aws-dead-letter-queue-1'
        }));
        expect(response.QueueUrl).toBe("https://nexq.local:7890/sqs/aws-dead-letter-queue-1");
        deadLetterUrl = response.QueueUrl ?? '';
    });

    test('create queue with attributes and tags', async () => {
        const response = await sqsClient.send(new CreateQueueCommand({
            QueueName: 'aws-queue-1',
            Attributes: {
                MessageRetentionPeriod: '100',
                RedrivePolicy: '{"deadLetterTargetArn":"arn:aws:sqs:test:test:aws-dead-letter-queue-1","maxReceiveCount":"1"}',
            },
            tags: { foo: "bar", fizz: "buzz" },
        }));
        expect(response.QueueUrl).toBe("https://nexq.local:7890/sqs/aws-queue-1");
        queue1Url = response.QueueUrl ?? '';
    });

    test('get queue attributes', async () => {
        const attributes = (await sqsClient.send(new GetQueueAttributesCommand({ QueueUrl: queue1Url }))).Attributes;
        if (!attributes) {
            throw new Error("missing attributes");
        }
        expect(attributes['ApproximateNumberOfMessages']).toBeTruthy();
        expect(attributes['ApproximateNumberOfMessagesDelayed']).toBeTruthy();
        expect(attributes['ApproximateNumberOfMessagesNotVisible']).toBeTruthy();
        expect(attributes['CreatedTimestamp']).toBeTruthy();
        expect(attributes['LastModifiedTimestamp']).toBeTruthy();
        expect(attributes["MaximumMessageSize"]).toBe('262144');
        expect(attributes["MessageRetentionPeriod"]).toBe('100');
        expect(attributes["QueueArn"]).toBe('arn:aws:sqs:test:test:aws-queue-1');
        expect(attributes["VisibilityTimeout"]).toBe('30');
        expect(attributes["RedrivePolicy"]).toBe('{"deadLetterTargetArn":"arn:aws:sqs:test:test:aws-dead-letter-queue-1","maxReceiveCount":"1"}');
    });

    test('get queues', async () => {
        const response = await sqsClient.send(new ListQueuesCommand({}));
        expect(response.QueueUrls?.length).toBe(2);
        let foundDeadLetter = false;
        let foundQueue = false;
        for (let queueUrl of response.QueueUrls ?? []) {
            if (queueUrl === "https://nexq.local:7890/sqs/aws-dead-letter-queue-1") {
                foundDeadLetter = true;
            } else if (queueUrl == "https://nexq.local:7890/sqs/aws-queue-1") {
                foundQueue = true
            } else {
                throw new Error(`found unexpected queue ${queueUrl}`)
            }
        }
        expect(foundDeadLetter).toBeTruthy();
        expect(foundQueue).toBeTruthy();
    });

    test('purge queue', async () => {
        await sqsClient.send(new SendMessageCommand({
            QueueUrl: queue1Url,
            MessageBody: messageTest1Body
        }));
        expect(await getQueueNumberOfMessages(sqsClient, queue1Url)).toBeGreaterThan(0);
        await sqsClient.send(new PurgeQueueCommand({
            QueueUrl: queue1Url
        }));
        expect(await getQueueNumberOfMessages(sqsClient, queue1Url)).toBe(0);
    });

    test('send message', async () => {
        const results = await sqsClient.send(new SendMessageCommand({
            QueueUrl: queue1Url,
            DelaySeconds: 0,
            MessageBody: messageTest1Body,
            MessageAttributes: {
                "attribute1": { "StringValue": "attribute1Value", "DataType": "String" }
            },
        }));
        expect(results.MD5OfMessageBody).toBe(messageTest1Md5OfMessageBody);
    });

    test('receive messages', async () => {
        const messages = await sqsClient.send(new ReceiveMessageCommand({
            QueueUrl: queue1Url,
            MaxNumberOfMessages: 1,
            VisibilityTimeout: 10,
            WaitTimeSeconds: 10,
            AttributeNames: [
                'ApproximateNumberOfMessages',
                "All",
            ],
            MessageAttributeNames: ["All"],
        }));
        expect(messages.Messages?.length).toBe(1);
        const message = messages.Messages?.[0];
        if (!message) {
            throw new Error('no message 0');
        }
        expect(message.Body).toBe(messageTest1Body);
        expect(message.MD5OfBody).toBe(messageTest1Md5OfMessageBody);
        expect(message.Attributes?.ApproximateFirstReceiveTimestamp).toBeTruthy();
        expect(message.Attributes?.ApproximateReceiveCount).toBe('1');
        expect(message.Attributes?.SentTimestamp).toBeTruthy();
        expect(message.Attributes?.SequenceNumber).toBeTruthy();
        expect(message.MessageAttributes?.['attribute1']?.StringValue).toBe('attribute1Value');

        await sqsClient.send(new ChangeMessageVisibilityCommand({
            QueueUrl: queue1Url,
            VisibilityTimeout: 10,
            ReceiptHandle: message.ReceiptHandle
        }));

        await sqsClient.send(new DeleteMessageCommand({
            QueueUrl: queue1Url,
            ReceiptHandle: message.ReceiptHandle
        }));

        const noMessages = await sqsClient.send(new ReceiveMessageCommand({
            QueueUrl: queue1Url,
            MaxNumberOfMessages: 1,
            VisibilityTimeout: 10,
            WaitTimeSeconds: 0,
        }));
        expect(noMessages.Messages?.length).toBe(0);
    });

    test('delete queue', async () => {
        await sqsClient.send(new DeleteQueueCommand({ QueueUrl: queue1Url }));
        const queueUrls = await sqsClient.send(new ListQueuesCommand({}));
        expect(queueUrls.QueueUrls?.includes(queue1Url)).toBeFalsy();
    });
});
