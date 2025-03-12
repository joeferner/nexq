import { afterAll, beforeAll, describe, expect, test, } from '@jest/globals';
import { SQSClient, CreateQueueCommand, GetQueueAttributesCommand, ReceiveMessageCommand, ReceiveMessageCommandOutput } from "@aws-sdk/client-sqs";
import { deleteAllQueues, deleteAllTopics, snsClientOpts, sqsClientOpts } from './utils';
import { CreateTopicCommand, PublishCommand, SNSClient, SubscribeCommand } from '@aws-sdk/client-sns';

const MESSAGE_BODY = 'test message 1';

describe('sns', () => {
    const sqsClient = new SQSClient(sqsClientOpts);
    const snsClient = new SNSClient(snsClientOpts);
    let topicArn = '';
    let queue1Url = '';
    let queue1Arn = '';
    let queue2Url = '';
    let queue2Arn = '';

    beforeAll(async () => {
        try {
            const resp1 = await sqsClient.send(new CreateQueueCommand({
                QueueName: 'sns-test-queue-1'
            }));
            const resp1Attrs = await sqsClient.send(new GetQueueAttributesCommand({
                QueueUrl: resp1.QueueUrl
            }));
            queue1Url = resp1.QueueUrl ?? '';
            queue1Arn = resp1Attrs.Attributes?.QueueArn ?? '';

            const resp2 = await sqsClient.send(new CreateQueueCommand({
                QueueName: 'sns-test-queue-2'
            }));
            const resp2Attrs = await sqsClient.send(new GetQueueAttributesCommand({
                QueueUrl: resp2.QueueUrl
            }));
            queue2Url = resp2.QueueUrl ?? '';
            queue2Arn = resp2Attrs.Attributes?.QueueArn ?? '';
        } catch (e) {
            console.log((e as any).$response);
            throw e;
        }
    });

    afterAll(async () => {
        await deleteAllTopics(snsClient);
        await deleteAllQueues(sqsClient);
    });

    test('create topic', async () => {
        const response = await snsClient.send(new CreateTopicCommand({
            Name: 'sns-test-topic-1',
            Tags: [{
                Key: 'tag1',
                Value: 'value1'
            }, {
                Key: 'tag2',
                Value: 'value2'
            }]
        }));
        expect(response.TopicArn).toBe("arn:aws:sns:test:test:sns-test-topic-1");
        topicArn = response.TopicArn ?? '';
    });

    test('subscribe SQS queues to topic', async () => {
        const response1 = await snsClient.send(new SubscribeCommand({
            TopicArn: topicArn,
            Protocol: 'sqs',
            Endpoint: queue1Arn
        }));
        expect(response1.SubscriptionArn).toBeTruthy();

        const response2 = await snsClient.send(new SubscribeCommand({
            TopicArn: topicArn,
            Protocol: 'sqs',
            Endpoint: queue2Arn
        }));
        expect(response2.SubscriptionArn).toBeTruthy();
    });

    test('publish message to topic', async () => {
        await snsClient.send(new PublishCommand({
            TopicArn: topicArn,
            Message: MESSAGE_BODY,
            MessageAttributes: {
                'attr1': {
                    DataType: 'String',
                    StringValue: 'attr1Value'
                },
                'attr2': {
                    DataType: 'String',
                    StringValue: 'attr2Value'
                }
            }
        }))
    });

    test('receive message from queues', async () => {
        const verifyResponse = (resp: ReceiveMessageCommandOutput) => {
            expect(resp.Messages?.length).toBe(1);
            const message = resp.Messages?.[0];
            if (!message) {
                throw new Error('missing message');
            }
            expect(message.Body).toBe(MESSAGE_BODY);
            const messageAttributes = message.MessageAttributes ?? {};
            expect(Object.keys(messageAttributes).length).toBe(2);
            expect(messageAttributes['attr1']?.StringValue).toBe('attr1Value');
            expect(messageAttributes['attr2']?.StringValue).toBe('attr2Value');
        };

        const resp1 = await sqsClient.send(new ReceiveMessageCommand({
            QueueUrl: queue1Url,
            MaxNumberOfMessages: 10,
            WaitTimeSeconds: 3,
            VisibilityTimeout: 10,
            MessageAttributeNames: ["All"]
        }));
        verifyResponse(resp1);

        const resp2 = await sqsClient.send(new ReceiveMessageCommand({
            QueueUrl: queue2Url,
            MaxNumberOfMessages: 10,
            WaitTimeSeconds: 3,
            VisibilityTimeout: 10,
            MessageAttributeNames: ["All"]
        }));
        verifyResponse(resp2);
    });
});
