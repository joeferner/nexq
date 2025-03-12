import { DeleteTopicCommand, ListTopicsCommand, SNSClient, SNSClientConfig } from "@aws-sdk/client-sns";
import { DeleteQueueCommand, GetQueueAttributesCommand, ListQueuesCommand, SQSClient, SQSClientConfig } from "@aws-sdk/client-sqs";
import https from 'node:https';
import fs from 'node:fs';
import { NodeHttpHandler } from "@smithy/node-http-handler";

const clientOpts: SQSClientConfig | SNSClientConfig = {
    region: "test",
    credentials: {
        accessKeyId: 'key',
        secretAccessKey: 'secret'
    },
    requestHandler: new NodeHttpHandler({
        httpsAgent: new https.Agent({
            rejectUnauthorized: false,
            key: fs.readFileSync('../../../certs/nexq.local.key'),
            cert: fs.readFileSync('../../../certs/nexq.local.crt'),
            ca: fs.readFileSync('../../../certs/nexq-rootca.crt')
        })
    })
};

export const sqsClientOpts = { ...clientOpts, endpoint: 'https://nexq.local:7890/sqs' };
export const snsClientOpts = { ...clientOpts, endpoint: 'https://nexq.local:7890/sns' };

export async function deleteAllQueues(sqsClient: SQSClient): Promise<void> {
    const response = await sqsClient.send(new ListQueuesCommand({}));
    for (let queueUrl of response.QueueUrls ?? []) {
        await sqsClient.send(new DeleteQueueCommand({ QueueUrl: queueUrl }));
    }
}

export async function deleteAllTopics(snsClient: SNSClient): Promise<void> {
    const response = await snsClient.send(new ListTopicsCommand({}));
    for (let topicArn of response.Topics?.map(t => t.TopicArn) ?? []) {
        await snsClient.send(new DeleteTopicCommand({ TopicArn: topicArn }));
    }
}

export async function getQueueNumberOfMessages(sqsClient: SQSClient, queueUrl: string): Promise<number> {
    const resp = await sqsClient.send(new GetQueueAttributesCommand({
        QueueUrl: queueUrl,
        AttributeNames: ['ApproximateNumberOfMessages']
    }));
    if (!resp.Attributes) {
        throw new Error('missing resp.Attributes');
    }
    if (resp.Attributes.ApproximateNumberOfMessages === undefined) {
        throw new Error('missing resp.Attributes.ApproximateNumberOfMessages');
    }
    return parseInt(resp.Attributes.ApproximateNumberOfMessages);
}
