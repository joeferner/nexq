from botocore.exceptions import ClientError
from utils import *
import pytest

message_test1_body = "test1 !@#$$%^&*()_+:/"
message_test1_md5_of_message_body = "0fa181988e7c9840758db6e0f3405f0c"

sqs = get_sqs_resource()
sns = get_sns_resource()


@pytest.fixture(scope="module")
def data():
    queue1 = sqs.create_queue(
        QueueName="sns-test-queue-1",
    )
    queue2 = sqs.create_queue(
        QueueName="sns-test-queue-2",
    )
    yield (queue1, queue2)
    delete_all_topics(sns)
    delete_all_queues(sqs)


def test_sns_create_topic(data):
    global topic
    topic = sns.create_topic(
        Name="sns-test-topic-1",
        Tags=[{"Key": "tag1", "Value": "value1"}, {"Key": "tag2", "Value": "value2"}],
    )


def test_sns_subscribe(data):
    (queue1, queue2) = data
    resp = topic.subscribe(Protocol="sqs", Endpoint=queue1.attributes["QueueArn"])
    assert_true(resp.arn)

    resp = topic.subscribe(Protocol="sqs", Endpoint=queue2.attributes["QueueArn"])
    assert_true(resp.arn)


def test_sns_publish_message(data):
    topic.publish(
        Message=message_test1_body,
        MessageAttributes={
            "attr1": {"DataType": "String", "StringValue": "attr1Value"},
            "attr2": {"DataType": "String", "StringValue": "attr2Value"},
        },
    )


def test_sns_receive_messages_from_queue(data):
    (queue1, queue2) = data

    def receive(queue):
        messages = queue.receive_messages(
            MaxNumberOfMessages=10,
            VisibilityTimeout=10,
            WaitTimeSeconds=10,
            MessageAttributeNames=["All"],
        )
        assert_equals(1, len(messages))
        message = messages[0]
        assert_equals(message_test1_body, message.body)
        assert_equals(message_test1_md5_of_message_body, message.md5_of_body)
        assert_equals("attr1Value", message.message_attributes["attr1"]["StringValue"])
        assert_equals("attr2Value", message.message_attributes["attr2"]["StringValue"])

    receive(queue1)
    receive(queue2)
