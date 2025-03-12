from botocore.config import Config
import boto3


def assert_equals(expected, found):
    if expected != found:
        raise Exception(f'assert_equals: expected "{expected}", found "{found}"')


def assert_contains(s, search):
    if search not in s:
        raise Exception(f'expected string "{s}" to contain "{search}"')


def assert_true(v, *args):
    if not v:
        if len(args) > 0:
            raise Exception(f"expected true: {args[0]}")
        raise Exception("expected true")


def fail(message):
    raise Exception(f"fail: {message}")


def get_queue_number_of_messages(queue):
    return int(queue.attributes["ApproximateNumberOfMessages"])


def delete_all_topics(sns):
    topics = list(sns.topics.all())
    for topic in topics:
        topic.delete()


def delete_all_queues(sqs):
    queues = list(sqs.queues.all())
    for queue in queues:
        queue.delete()


def get_resource(type):
    config = Config(
        client_cert=("../../../certs/nexq.local.crt", "../../../certs/nexq.local.key")
    )

    # boto3.set_stream_logger(name='botocore')
    return boto3.resource(
        type,
        region_name="test",
        endpoint_url=f"https://nexq.local:7890/{type}",
        aws_access_key_id="key",
        aws_secret_access_key="secret",
        config=config,
        verify="../../../certs/nexq-rootca.crt",
    )


def get_sqs_resource():
    return get_resource("sqs")


def get_sns_resource():
    return get_resource("sns")
