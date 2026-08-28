//! Both facades in one process, over one engine.
//!
//! The claim this file exists to prove: a message sent through the SQS facade is
//! receivable through the REST facade, because the two are translation layers over a
//! single [`Engine`] rather than two queueing systems that happen to share a binary.
//!
//! Deliberately end to end over real sockets, not `oneshot` against the routers. Two
//! bound listeners is the thing being claimed, and a test that skipped the sockets would
//! still pass if `nexq-server` never bound the second one.
//!
//! Signing is why this reaches for `nexq-api-aws`: only that crate knows how to produce a
//! SigV4 signature its own verifier will accept, and sending the message any other way
//! would test something weaker than "sent through the SQS facade".

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, Method, Uri, header};
use nexq_api_aws::sigv4::{ALGORITHM, Authorization, CredentialScope, SigningContext};
use nexq_core::engine::Engine;
use nexq_core::store::Store;
use nexq_core::{AuthConfig, AwsApiConfig, Credential, RestApiConfig, Secret};
use nexq_store_memory::MemoryStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;

const KEY_ID: &str = "AKIATESTKEY";
const SECRET: &str = "test-secret";
const REGION: &str = "us-east-1";
const QUEUE: &str = "shared";

/// Fixed, which is why the facade below is configured with the freshness check off. The
/// window itself is covered by `nexq-api-aws`; what this file is about is the engine
/// being shared.
const AMZ_DATE: &str = "20260826T005924Z";
const SCOPE_DATE: &str = "20260826";

fn credential() -> Credential {
    Credential {
        name: "dev".to_owned(),
        key_id: KEY_ID.to_owned(),
        secret: Secret::new(SECRET),
    }
}

fn bearer_token() -> String {
    credential().bearer_token()
}

/// One engine, one registry, two listeners — the arrangement `nexq-server` builds.
async fn both_facades() -> (SocketAddr, SocketAddr) {
    let (aws, rest, shutdown) = both_facades_with_shutdown().await;

    // Leaked rather than dropped, so serving continues for the rest of the test: dropping
    // the sender closes the channel, which resolves every receiver and starts the drain.
    // A test process is about to exit, so one leaked sender costs nothing.
    std::mem::forget(shutdown);

    (aws, rest)
}

/// The same, plus the handle that signals graceful shutdown.
async fn both_facades_with_shutdown() -> (SocketAddr, SocketAddr, watch::Sender<bool>) {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let engine = Arc::new(Engine::new(store));
    let auth = Arc::new(AuthConfig {
        credentials: vec![credential()],
    });

    let aws = nexq_api_aws::Server::bind(
        &AwsApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            // These requests are signed with a fixed timestamp.
            max_clock_skew_secs: 0,
            ..AwsApiConfig::default()
        },
        Arc::clone(&auth),
        Arc::clone(&engine),
    )
    .await
    .expect("bind the aws facade");

    let rest = nexq_api_rest::Server::bind(
        &RestApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..RestApiConfig::default()
        },
        Arc::clone(&auth),
        Arc::clone(&engine),
    )
    .await
    .expect("bind the rest facade");

    let (aws_addr, rest_addr) = (aws.local_addr(), rest.local_addr());

    // One signal, both facades — the shape `nexq-server` uses, and the reason a drain
    // started by either releases waiters parked on the other.
    //
    // A `watch` rather than a `Notify`, for the same reason `nexq-server` uses one: it is
    // level-triggered, so a facade that has not reached its await yet when the signal
    // fires still sees it. `notify_waiters` would lose that wake and the test would hang
    // rather than fail.
    let (tx, _) = watch::channel(false);

    tokio::spawn(aws.serve(stop_on(tx.subscribe())));
    tokio::spawn(rest.serve(stop_on(tx.subscribe())));

    (aws_addr, rest_addr, tx)
}

/// Resolves once shutdown is signalled.
async fn stop_on(mut shutdown: watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// The REST facade **alone**, with the engine it runs over and its shutdown handle.
///
/// Alone on purpose. With both facades over one engine, either one's drain releases the
/// other's waiters, so a test that ran both could not tell which facade had done it —
/// breaking REST's own drain on purpose left such a test green, which is how this helper
/// came to exist. Queue setup goes through the engine directly because REST has no
/// create-queue route yet, and what is under test here is the shutdown path.
async fn rest_only_with_shutdown() -> (SocketAddr, Arc<Engine>, watch::Sender<bool>) {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let engine = Arc::new(Engine::new(store));
    let auth = Arc::new(AuthConfig {
        credentials: vec![credential()],
    });

    let rest = nexq_api_rest::Server::bind(
        &RestApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..RestApiConfig::default()
        },
        auth,
        Arc::clone(&engine),
    )
    .await
    .expect("bind the rest facade");

    let address = rest.local_addr();
    let (tx, _) = watch::channel(false);
    tokio::spawn(rest.serve(stop_on(tx.subscribe())));

    (address, engine, tx)
}

/// Send one HTTP/1.1 request and read the whole response.
///
/// `Connection: close` so the read ends at the response rather than waiting on a
/// keep-alive connection the server has no reason to close.
async fn round_trip(address: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        stream.read_to_string(&mut response),
    )
    .await
    .expect("the server should answer rather than hold the connection")
    .expect("read response");

    response
}

/// The JSON body of a response, with the status asserted first so a failure reports the
/// status rather than a parse error about an error page.
fn body_of(response: &str, expected_status: &str) -> serde_json::Value {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {expected_status}")),
        "expected {expected_status}, got:\n{response}"
    );

    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("a response has a body");

    serde_json::from_str(body).unwrap_or_else(|error| panic!("body was not JSON: {error}\n{body}"))
}

/// Sign a request the way botocore does, and render it as raw HTTP.
///
/// Every header sent is a signed header, which is stricter than the CLI's own choice and
/// means a header added here without being signed would fail rather than pass silently.
fn signed_sqs_request(address: SocketAddr, target: &str, body: &str) -> String {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HOST,
        HeaderValue::from_str(&address.to_string()).expect("host"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-amz-json-1.0"),
    );
    headers.insert("x-amz-date", HeaderValue::from_static(AMZ_DATE));
    headers.insert(
        "x-amz-target",
        HeaderValue::from_str(target).expect("target"),
    );

    let mut names: Vec<String> = headers
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect();
    names.sort();

    let uri: Uri = "/".parse().expect("uri");
    let signature = nexq_api_aws::sigv4::sign(
        &SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: body.as_bytes(),
        },
        &Authorization {
            key_id: KEY_ID.to_owned(),
            scope: CredentialScope {
                date: SCOPE_DATE.to_owned(),
                region: REGION.to_owned(),
                service: nexq_api_aws::sigv4::SERVICE.to_owned(),
            },
            signed_headers: names.clone(),
            signature: String::new(),
        },
        AMZ_DATE,
        &credential(),
    )
    .expect("sign");

    let mut request = String::from("POST / HTTP/1.1\r\n");
    for (name, value) in &headers {
        request.push_str(&format!(
            "{name}: {}\r\n",
            value.to_str().expect("header value")
        ));
    }
    request.push_str(&format!(
        "authorization: {ALGORITHM} Credential={KEY_ID}/{SCOPE_DATE}/{REGION}/{}/aws4_request, \
         SignedHeaders={}, Signature={signature}\r\n",
        nexq_api_aws::sigv4::SERVICE,
        names.join(";")
    ));
    request.push_str(&format!("content-length: {}\r\n", body.len()));
    request.push_str("connection: close\r\n\r\n");
    request.push_str(body);

    request
}

fn rest_receive_request(address: SocketAddr, queue: &str, body: &str) -> String {
    rest_request(address, queue, "receive", body)
}

fn rest_send_request(address: SocketAddr, queue: &str, body: &str) -> String {
    rest_request(address, queue, "", body)
}

/// A signed-in REST request against one queue's message collection, or an action on it.
fn rest_request(address: SocketAddr, queue: &str, action: &str, body: &str) -> String {
    format!(
        "POST /api/v1/queues/{queue}/messages{}{action} HTTP/1.1\r\n\
         Host: {address}\r\n\
         Authorization: Bearer {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        if action.is_empty() { "" } else { "/" },
        bearer_token(),
        body.len()
    )
}

/// Create `QUEUE` over SQS and return its URL.
async fn create_queue_over_sqs(aws: SocketAddr) -> String {
    let created = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.CreateQueue",
            &format!(r#"{{"QueueName":"{QUEUE}"}}"#),
        ),
    )
    .await;

    body_of(&created, "200")["QueueUrl"]
        .as_str()
        .expect("a queue url")
        .to_owned()
}

/// The item's claim, end to end: created and sent over SQS, received over REST.
#[tokio::test]
async fn a_message_sent_over_sqs_is_receivable_over_rest() {
    let (aws, rest) = both_facades().await;
    let queue_url = create_queue_over_sqs(aws).await;

    let sent = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.SendMessage",
            &format!(r#"{{"QueueUrl":"{queue_url}","MessageBody":"across the facades"}}"#),
        ),
    )
    .await;
    let sent_id = body_of(&sent, "200")["MessageId"]
        .as_str()
        .expect("a message id")
        .to_owned();

    // The other listener, the other protocol, the same queue.
    let received = round_trip(rest, &rest_receive_request(rest, QUEUE, "{}")).await;
    let messages = &body_of(&received, "200")["messages"];

    assert_eq!(messages[0]["body"], "across the facades");
    assert_eq!(
        messages[0]["id"], sent_id,
        "the same message, not merely a message with the same body"
    );
    assert_eq!(messages[0]["receiveCount"], 1);
    assert!(
        messages[0]["claimExpiresInSeconds"]
            .as_u64()
            .is_some_and(|seconds| seconds > 0),
        "a claim was made, so it has time left: {messages}"
    );
}

/// Priority is one concept, not one per facade: what an SQS client sets with the
/// `NexQ.Priority` message attribute is what REST reports on the same message.
///
/// Over real sockets and a real signature for the same reason the rest of this file is:
/// the unit tests in `nexq-api-aws` prove the attribute is read, and this proves the value
/// they read is the one the *other* facade serves.
#[tokio::test]
async fn a_priority_set_by_an_sqs_client_is_the_priority_rest_reports() {
    let (aws, rest) = both_facades().await;
    let queue_url = create_queue_over_sqs(aws).await;

    let sent = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.SendMessage",
            &format!(
                r#"{{"QueueUrl":"{queue_url}","MessageBody":"urgent","MessageAttributes":{{
                     "NexQ.Priority":{{"DataType":"Number","StringValue":"10"}}}}}}"#
            ),
        ),
    )
    .await;
    let sent = body_of(&sent, "200");

    let received = round_trip(rest, &rest_receive_request(rest, QUEUE, "{}")).await;
    let message = &body_of(&received, "200")["messages"][0];

    assert_eq!(
        message["id"], sent["MessageId"],
        "the same message, not merely one with the same body"
    );
    assert_eq!(
        message["priority"], 10,
        "an SQS client chose it, and REST reports it: {message}"
    );
    assert_eq!(
        message["attributes"]["NexQ.Priority"],
        serde_json::json!({ "type": "number", "value": "10" }),
        "and the attribute itself is still there, since an SDK checksums what it sent"
    );
}

/// The two facades reserve the same attribute namespace, and this is what keeps the two
/// spellings of it equal — the constants live in different crates, and neither can see the
/// other's outside a test that depends on both.
///
/// The reservation matters in opposite directions. On the SQS facade `NexQ.Priority` is
/// *the* way to set a priority, so a name near it must not pass as ordinary metadata; on
/// REST, where priority is a field, the attribute must not exist at all or a message could
/// carry two different answers.
#[tokio::test]
async fn the_two_facades_reserve_the_same_namespace() {
    let (aws, rest) = both_facades().await;
    create_queue_over_sqs(aws).await;

    let attribute = nexq_api_aws::message_attributes::PRIORITY;
    assert!(
        attribute.to_ascii_lowercase().starts_with("nexq."),
        "{attribute} should be in the namespace REST refuses"
    );

    let refused = round_trip(
        rest,
        &rest_send_request(
            rest,
            QUEUE,
            &format!(
                r#"{{"messages":[{{"body":"x","attributes":{{
                     "{attribute}":{{"type":"number","value":"10"}}}}}}]}}"#
            ),
        ),
    )
    .await;

    // A `200` carrying a refused entry, since a send is not a transaction and the
    // attribute belongs to one message rather than to the request.
    let entry = &body_of(&refused, "200")["results"][0];
    assert_eq!(entry["status"], "refused");

    let error = &entry["error"];
    assert_eq!(error["code"], "invalid_message_attribute");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("priority")),
        "the error should point at the field to use instead: {error}"
    );
}

/// The converse: a priority set over REST is readable by an SQS consumer, which is the
/// only reason the facade answers it as a system attribute at all — a message sent through
/// REST carries no priority *attribute* for a consumer to read.
#[tokio::test]
async fn a_priority_set_over_rest_is_readable_by_an_sqs_consumer() {
    let (aws, rest) = both_facades().await;
    let queue_url = create_queue_over_sqs(aws).await;

    let sent = round_trip(
        rest,
        &rest_send_request(
            rest,
            QUEUE,
            r#"{"messages":[{"body":"from rest","priority":3}]}"#,
        ),
    )
    .await;
    assert_eq!(body_of(&sent, "200")["results"][0]["status"], "accepted");

    let received = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.ReceiveMessage",
            &format!(
                r#"{{"QueueUrl":"{queue_url}",
                     "MessageSystemAttributeNames":["NexQ.Priority"],
                     "MessageAttributeNames":["All"]}}"#
            ),
        ),
    )
    .await;
    let message = &body_of(&received, "200")["Messages"][0];

    assert_eq!(message["Body"], "from rest");
    assert_eq!(message["Attributes"]["NexQ.Priority"], "3");
    assert!(
        message.get("MessageAttributes").is_none(),
        "nothing is invented in the producer's own attribute map: {message}"
    );
}

/// The converse, and the reason it matters: the claim is shared too. A message REST has
/// claimed must not also be handed to an SQS consumer, or the two facades would be two
/// queues that happen to hold the same messages.
#[tokio::test]
async fn a_message_claimed_over_rest_is_invisible_to_sqs() {
    let (aws, rest) = both_facades().await;

    round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.CreateQueue",
            &format!(r#"{{"QueueName":"{QUEUE}"}}"#),
        ),
    )
    .await;
    let created = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.GetQueueUrl",
            &format!(r#"{{"QueueName":"{QUEUE}"}}"#),
        ),
    )
    .await;
    let queue_url = body_of(&created, "200")["QueueUrl"]
        .as_str()
        .expect("a queue url")
        .to_owned();

    round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.SendMessage",
            &format!(r#"{{"QueueUrl":"{queue_url}","MessageBody":"claimed by rest"}}"#),
        ),
    )
    .await;

    // A long visibility timeout, so the claim cannot lapse during the test.
    let received = round_trip(
        rest,
        &rest_receive_request(rest, QUEUE, r#"{"visibilityTimeoutSeconds": 300}"#),
    )
    .await;
    assert_eq!(
        body_of(&received, "200")["messages"][0]["body"],
        "claimed by rest"
    );

    let sqs_receive = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.ReceiveMessage",
            &format!(r#"{{"QueueUrl":"{queue_url}"}}"#),
        ),
    )
    .await;

    // SQS omits `Messages` entirely when there are none, which is the shape being
    // asserted: the message is claimed, not merely absent from this response's list.
    assert!(
        body_of(&sqs_receive, "200").get("Messages").is_none(),
        "REST holds the claim, so SQS must see nothing:\n{sqs_receive}"
    );
}

/// Both facades are bound, and they are not bound to the same place.
#[tokio::test]
async fn each_facade_gets_its_own_listener() {
    let (aws, rest) = both_facades().await;

    assert_ne!(aws, rest, "two facades, two listeners");

    // Neither serves the other's routes, so a request sent to the wrong port is a 404
    // rather than something half-understood. Note *where* each refusal comes from: this
    // is routing, not authentication — neither request gets far enough to have its
    // credentials looked at, which is why a valid bearer token on the SQS port and a
    // valid signature on the REST port both come back as "no such route".
    let to_aws = round_trip(aws, &rest_receive_request(aws, QUEUE, "{}")).await;
    assert!(
        to_aws.starts_with("HTTP/1.1 404"),
        "the SQS facade serves POST / only:\n{to_aws}"
    );

    let to_rest = round_trip(
        rest,
        &signed_sqs_request(rest, "AmazonSQS.ListQueues", "{}"),
    )
    .await;
    assert_eq!(
        body_of(&to_rest, "404")["error"]["code"],
        "no_such_route",
        "and REST answers in its own envelope even for a path it does not have"
    );
}

/// Long polling is one mechanism with two protocol faces, not two implementations: the
/// REST wait parks on `nexq-core`'s waiter registry, and an **SQS** send is what wakes it.
///
/// The strongest available proof that the registry is shared — a second waiter registry
/// would leave this poll to time out at its own deadline instead.
#[tokio::test]
async fn an_sqs_send_wakes_a_rest_long_poll() {
    let (aws, rest) = both_facades().await;

    round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.CreateQueue",
            &format!(r#"{{"QueueName":"{QUEUE}"}}"#),
        ),
    )
    .await;
    let created = round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.GetQueueUrl",
            &format!(r#"{{"QueueName":"{QUEUE}"}}"#),
        ),
    )
    .await;
    let queue_url = body_of(&created, "200")["QueueUrl"]
        .as_str()
        .expect("a queue url")
        .to_owned();

    // Parked on an empty queue for ten seconds.
    let request = rest_receive_request(rest, QUEUE, r#"{"waitTimeSeconds": 10}"#);
    let started = std::time::Instant::now();
    let poll = tokio::spawn(async move { round_trip(rest, &request).await });

    // Enough for the poll to reach the engine and register, so the wake is what returns
    // it rather than the claim it makes on its way in.
    tokio::time::sleep(Duration::from_millis(300)).await;
    round_trip(
        aws,
        &signed_sqs_request(
            aws,
            "AmazonSQS.SendMessage",
            &format!(r#"{{"QueueUrl":"{queue_url}","MessageBody":"woke the poller"}}"#),
        ),
    )
    .await;

    let received = poll.await.expect("the poll task should not panic");
    let elapsed = started.elapsed();

    assert_eq!(
        body_of(&received, "200")["messages"][0]["body"],
        "woke the poller"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the send should have woken the poll, but it took {elapsed:?} of its 10s deadline"
    );
}

/// Shutdown releases a REST long poll instead of waiting it out.
///
/// This is M4's 19.7-second lesson applied to the second facade: a consumer parked for
/// twenty seconds is an in-flight request, and a graceful shutdown that waits for
/// in-flight requests would wait for it. The waiter gets its normal empty answer.
#[tokio::test]
async fn shutting_down_releases_a_rest_long_poll() {
    let (rest, engine, shutdown) = rest_only_with_shutdown().await;

    engine
        .create_queue(
            nexq_core::QueueName::new(QUEUE).expect("valid name"),
            nexq_core::QueueAttributes::default(),
        )
        .await
        .expect("create the queue");

    let request = rest_receive_request(rest, QUEUE, r#"{"waitTimeSeconds": 20}"#);
    let started = std::time::Instant::now();
    let poll = tokio::spawn(async move { round_trip(rest, &request).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    shutdown.send(true).expect("signal shutdown");

    let received = poll.await.expect("the poll task should not panic");
    let elapsed = started.elapsed();

    assert_eq!(
        body_of(&received, "200")["messages"],
        serde_json::json!([]),
        "a released waiter gets the ordinary empty answer, not a dropped connection"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "shutdown should have released the poll, but it took {elapsed:?} of its 20s deadline"
    );
}
