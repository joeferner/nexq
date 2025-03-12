from botocore.exceptions import ClientError
from utils import *
import pytest

message_test1_body = "test1 !@#$$%^&*()_+:/"
message_test1_md5_of_message_body = "0fa181988e7c9840758db6e0f3405f0c"

sqs = get_sqs_resource()


@pytest.fixture(scope="module")
def data():
    yield ()
    delete_all_queues(sqs)


def test_sqs_create_dead_letter_queue(data):
    dead_letter_queue = sqs.create_queue(QueueName="aws-dead-letter-queue-1")
    assert_equals(
        "https://nexq.local:7890/sqs/aws-dead-letter-queue-1", dead_letter_queue.url
    )


def test_sqs_create_queue_with_attributes_and_tags(data):
    queue = sqs.create_queue(
        QueueName="aws-queue-1",
        Attributes={
            "MessageRetentionPeriod": "100",
            "RedrivePolicy": '{"deadLetterTargetArn":"arn:aws:sqs:test:test:aws-dead-letter-queue-1","maxReceiveCount":"2"}',
        },
        tags={"foo": "bar", "fizz": "buzz"},
    )
    assert_equals("https://nexq.local:7890/sqs/aws-queue-1", queue.url)


def test_sqs_get_queue_attributes(data):
    queue = sqs.get_queue_by_name(QueueName="aws-queue-1")
    attrs = queue.attributes
    assert_true("ApproximateNumberOfMessages" in attrs, "ApproximateNumberOfMessages")
    assert_true(
        "ApproximateNumberOfMessagesDelayed" in attrs,
        "ApproximateNumberOfMessagesDelayed",
    )
    assert_true(
        "ApproximateNumberOfMessagesNotVisible" in attrs,
        "ApproximateNumberOfMessagesNotVisible",
    )
    assert_true("CreatedTimestamp" in attrs, "CreatedTimestamp")
    assert_true("LastModifiedTimestamp" in attrs, "LastModifiedTimestamp")
    assert_equals("262144", attrs["MaximumMessageSize"])
    assert_equals("100", attrs["MessageRetentionPeriod"])
    assert_equals("arn:aws:sqs:test:test:aws-queue-1", attrs["QueueArn"])
    assert_equals("30", attrs["VisibilityTimeout"])
    assert_equals(
        '{"deadLetterTargetArn":"arn:aws:sqs:test:test:aws-dead-letter-queue-1","maxReceiveCount":"2"}',
        attrs["RedrivePolicy"],
    )


def test_sqs_create_queue_with_bad_attribute(data):
    try:
        sqs.create_queue(
            QueueName="aws-queue-bad-attribute-1",
            Attributes={"BadAttribute": "100"},
        )
        fail("should not create queue with bad attribute")
    except ClientError as error:
        assert_contains(str(error), "InvalidAttributeName")


def test_sqs_get_queues(data):
    queues = list(sqs.queues.all())
    assert_equals(2, len(queues))
    found_dead_letter = False
    found_queue = False
    for queue in queues:
        if queue.url == "https://nexq.local:7890/sqs/aws-dead-letter-queue-1":
            found_dead_letter = True
        elif queue.url == "https://nexq.local:7890/sqs/aws-queue-1":
            found_queue = True
        else:
            fail(f"found unexpected queue {queue.url}")
    assert_equals(True, found_dead_letter)
    assert_equals(True, found_queue)


def test_sqs_purge_queue(data):
    queue = sqs.get_queue_by_name(QueueName="aws-queue-1")
    queue.send_message(
        MessageBody=message_test1_body,
    )
    assert_true(get_queue_number_of_messages(queue) > 0)
    queue.purge()
    assert_equals(0, get_queue_number_of_messages(queue))


def test_sqs_send_message(data):
    queue = sqs.get_queue_by_name(QueueName="aws-queue-1")
    results = queue.send_message(
        DelaySeconds=1,
        MessageBody=message_test1_body,
        MessageAttributes={
            "attribute1": {"StringValue": "attribute1Value", "DataType": "String"}
        },
    )
    assert_equals(message_test1_md5_of_message_body, results["MD5OfMessageBody"])


def test_sqs_receive_messages(data):
    queue = sqs.get_queue_by_name(QueueName="aws-queue-1")
    messages = queue.receive_messages(
        MaxNumberOfMessages=1,
        VisibilityTimeout=10,
        WaitTimeSeconds=10,
        AttributeNames=[
            "ApproximateFirstReceiveTimestamp",
            "ApproximateReceiveCount",
            "All",
        ],
        MessageAttributeNames=["All"],
    )
    assert_equals(1, len(messages))
    message = messages[0]
    # see https://boto3.amazonaws.com/v1/documentation/api/latest/reference/services/sqs/message/index.html
    assert_equals(message_test1_body, message.body)
    assert_equals(message_test1_md5_of_message_body, message.md5_of_body)
    assert_true(
        "ApproximateFirstReceiveTimestamp" in message.attributes,
        "ApproximateFirstReceiveTimestamp",
    )
    assert_equals("1", message.attributes["ApproximateReceiveCount"])
    assert_true(
        "SentTimestamp" in message.attributes,
        "SentTimestamp",
    )
    assert_true(
        "SequenceNumber" in message.attributes,
        "SequenceNumber",
    )
    assert_equals(
        "attribute1Value", message.message_attributes["attribute1"]["StringValue"]
    )

    message.change_visibility(VisibilityTimeout=10)

    message.delete()

    messages = queue.receive_messages(
        MaxNumberOfMessages=1, VisibilityTimeout=10, WaitTimeSeconds=0
    )
    assert_equals(0, len(messages))


def test_sqs_delete_queue(data):
    queue = sqs.get_queue_by_name(QueueName="aws-queue-1")
    queue.delete()
