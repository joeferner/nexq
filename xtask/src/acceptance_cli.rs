//! The acceptance suite: the real `aws` CLI driving a real NexQ server.
//!
//! Distinct from the unit and conformance suites, and worth having despite the overlap.
//! Those test our code against our understanding of SQS. This tests it against a client
//! that has its own understanding — botocore signs its own requests, verifies its own
//! checksums, walks its own paginators, and decides for itself what an error means. A
//! check that passes here is evidence; a unit test agreeing with itself is not.
//!
//! Every check gets its own queue, so one failure cannot make the next check lie.
//!
//! Timing is asserted only where timing is the behaviour — a long poll returning early
//! rather than waiting out its deadline. Bounds are loose, because a shared runner is
//! slow and irregular and a flaky acceptance test is worse than a missing one.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::harness::{Aws, KEY_ID, Server};

/// One check: a name, and something to run against a server.
///
/// Every check returns rather than panicking, so a failure reports what it saw and the
/// rest still run — one broken thing should not hide the others.
type Check = (&'static str, fn(&Aws) -> Result<(), String>);

/// Run every check, reporting each and failing if any did not pass.
pub fn run() -> Result<(), String> {
    println!("starting nexq-server and driving it with the real aws CLI\n");

    let server = Server::start()?;
    let aws = server.aws();
    println!("  server at {}\n", server.endpoint);

    let checks: Vec<Check> = vec![
        ("queue lifecycle", queue_lifecycle),
        ("listing and paging", listing_and_paging),
        ("send, receive, delete", produce_and_consume),
        ("message attributes", message_attributes),
        ("system attributes", system_attributes),
        ("long polling", long_polling),
        ("visibility timeout", visibility_timeout),
        ("batch operations", batch_operations),
        ("queue attributes", queue_attributes),
        ("purge", purge),
        ("authentication", authentication),
        ("unimplemented operations", unimplemented_operations),
    ];

    let mut failures = Vec::new();
    for (name, check) in checks {
        match check(&aws) {
            Ok(()) => println!("  ok    {name}"),
            Err(reason) => {
                println!("  FAIL  {name}");
                println!("          {reason}");
                failures.push(name);
            }
        }
    }

    // TLS needs its own server, so it runs outside the loop above rather than being
    // given a way to swap the server out from under the other checks.
    match over_tls() {
        Ok(()) => println!("  ok    TLS"),
        Err(reason) => {
            println!("  FAIL  TLS");
            println!("          {reason}");
            failures.push("TLS");
        }
    }

    println!();
    if failures.is_empty() {
        println!("all checks passed");
        return Ok(());
    }

    Err(format!(
        "{} of the checks failed: {}",
        failures.len(),
        failures.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

fn queue_lifecycle(aws: &Aws) -> Result<(), String> {
    let created = aws.sqs(&["create-queue", "--queue-name", "acc-lifecycle"])?;
    let url = string(&created, "QueueUrl")?;

    if !url.ends_with("/acc-lifecycle") {
        return Err(format!("queue url looks wrong: {url}"));
    }

    // Idempotent with the same attributes, a conflict with different ones.
    aws.sqs(&["create-queue", "--queue-name", "acc-lifecycle"])?;
    let code = aws.sqs_err(&[
        "create-queue",
        "--queue-name",
        "acc-lifecycle",
        "--attributes",
        "VisibilityTimeout=600",
    ])?;
    expect(&code, "QueueNameExists")?;

    // The URL a lookup reports is the URL create handed out, which is what every later
    // request depends on.
    let looked_up = aws.sqs(&["get-queue-url", "--queue-name", "acc-lifecycle"])?;
    expect(&string(&looked_up, "QueueUrl")?, &url)?;

    aws.sqs(&["delete-queue", "--queue-url", &url])?;
    let code = aws.sqs_err(&["get-queue-url", "--queue-name", "acc-lifecycle"])?;
    expect(&code, "QueueDoesNotExist")
}

fn listing_and_paging(aws: &Aws) -> Result<(), String> {
    for index in 0..7 {
        aws.sqs(&["create-queue", "--queue-name", &format!("acc-page-{index}")])?;
    }

    let prefixed = aws.sqs(&["list-queues", "--queue-name-prefix", "acc-page-"])?;
    let count = array(&prefixed, "QueueUrls")?.len();
    if count != 7 {
        return Err(format!("expected 7 queues with the prefix, got {count}"));
    }

    // botocore's own paginator walking our NextToken, which is the point: the tokens
    // have to be usable by a client that was not written against them.
    let paged = aws.sqs(&[
        "list-queues",
        "--queue-name-prefix",
        "acc-page-",
        "--page-size",
        "2",
    ])?;
    let count = array(&paged, "QueueUrls")?.len();
    if count != 7 {
        return Err(format!(
            "the paginator aggregated {count} queues, expected 7"
        ));
    }

    // Stopping early must offer a way to continue.
    let partial = aws.sqs(&[
        "list-queues",
        "--queue-name-prefix",
        "acc-page-",
        "--max-items",
        "3",
    ])?;
    if array(&partial, "QueueUrls")?.len() != 3 {
        return Err("expected 3 queues from --max-items 3".to_owned());
    }
    if partial.get("NextToken").is_none() {
        return Err("no NextToken offered when queues remained".to_owned());
    }

    Ok(())
}

fn produce_and_consume(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-loop")?;

    let sent = aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "hello world",
    ])?;
    // The MD5 of "hello world", which the CLI itself has already checked by this point.
    expect(
        &string(&sent, "MD5OfMessageBody")?,
        "5eb63bbbe01eeed093cb22bb8f5acdc3",
    )?;
    let message_id = string(&sent, "MessageId")?;

    let received = aws.sqs(&["receive-message", "--queue-url", &url])?;
    let message = &array(&received, "Messages")?[0];
    expect(&as_str(&message["Body"])?, "hello world")?;
    expect(&as_str(&message["MessageId"])?, &message_id)?;
    expect(
        &as_str(&message["MD5OfBody"])?,
        "5eb63bbbe01eeed093cb22bb8f5acdc3",
    )?;

    let handle = as_str(&message["ReceiptHandle"])?;
    aws.sqs(&[
        "delete-message",
        "--queue-url",
        &url,
        "--receipt-handle",
        &handle,
    ])?;

    // Gone for good, and the spent handle is refused.
    let empty = aws.sqs(&["receive-message", "--queue-url", &url])?;
    if empty.get("Messages").is_some() {
        return Err("a deleted message came back".to_owned());
    }
    let code = aws.sqs_err(&[
        "delete-message",
        "--queue-url",
        &url,
        "--receipt-handle",
        &handle,
    ])?;
    expect(&code, "ReceiptHandleIsInvalid")
}

fn message_attributes(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-attrs")?;
    let attributes = json!({
        "City": { "DataType": "String", "StringValue": "Any City" },
        "Population": { "DataType": "Number", "StringValue": "1250800" },
        "Thumb": { "DataType": "Binary", "BinaryValue": "iVBORw0KGgo=" },
    })
    .to_string();

    let sent = aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "hello",
        "--message-attributes",
        &attributes,
    ])?;
    let sent_digest = string(&sent, "MD5OfMessageAttributes")?;

    let received = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--message-attribute-names",
        "All",
    ])?;
    let message = &array(&received, "Messages")?[0];

    // What went in is what comes out, binary included, and it checksums the same.
    let returned = message["MessageAttributes"]
        .as_object()
        .ok_or("no MessageAttributes came back")?;
    expect(&returned.len().to_string(), "3")?;
    expect(&as_str(&returned["City"]["StringValue"])?, "Any City")?;
    expect(&as_str(&returned["Thumb"]["BinaryValue"])?, "iVBORw0KGgo=")?;
    expect(&as_str(&message["MD5OfMessageAttributes"])?, &sent_digest)
}

fn system_attributes(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-system-attrs")?;
    aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "hello",
    ])?;

    // Received twice with a zero visibility timeout, so the second is a redelivery.
    for expected in ["1", "2"] {
        let received = aws.sqs(&[
            "receive-message",
            "--queue-url",
            &url,
            "--visibility-timeout",
            "0",
            "--message-system-attribute-names",
            "All",
        ])?;
        let attributes = &array(&received, "Messages")?[0]["Attributes"];

        expect(&as_str(&attributes["ApproximateReceiveCount"])?, expected)?;

        // Epoch milliseconds, so a plausible value is a large number rather than a
        // second-scale one — a units mistake would show up here.
        let sent: u64 = as_str(&attributes["SentTimestamp"])?
            .parse()
            .map_err(|_| "SentTimestamp is not a number".to_owned())?;
        if sent < 1_000_000_000_000 {
            return Err(format!("SentTimestamp {sent} is not epoch milliseconds"));
        }
    }

    Ok(())
}

fn long_polling(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-long-poll")?;

    // An empty queue with a short wait: the answer is empty, and it took the wait.
    let started = Instant::now();
    let empty = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--wait-time-seconds",
        "2",
    ])?;
    if empty.get("Messages").is_some() {
        return Err("an empty queue returned a message".to_owned());
    }
    if started.elapsed() < Duration::from_secs(2) {
        return Err(format!(
            "a 2 second wait returned after {:?}, so it did not wait",
            started.elapsed()
        ));
    }

    // The gate: a receive asking for twenty seconds must return when a message is sent
    // from elsewhere, not when its deadline runs out.
    let sender = {
        let aws = aws.clone();
        let url = url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            aws.sqs(&[
                "send-message",
                "--queue-url",
                &url,
                "--message-body",
                "from another terminal",
            ])
        })
    };

    let started = Instant::now();
    let received = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--wait-time-seconds",
        "20",
    ])?;
    let waited = started.elapsed();
    sender
        .join()
        .map_err(|_| "the sending thread panicked".to_owned())??;

    expect(
        &as_str(&array(&received, "Messages")?[0]["Body"])?,
        "from another terminal",
    )?;

    // Loose on purpose: the send happens 3 seconds in, and the CLI itself takes about a
    // second to start. Anything under 15 proves it was woken rather than timed out.
    if waited > Duration::from_secs(15) {
        return Err(format!(
            "the long poll took {waited:?}, so it waited out its deadline instead of \
             being woken by the send"
        ));
    }

    Ok(())
}

fn visibility_timeout(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-visibility")?;
    aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "work",
    ])?;

    // Claimed for twelve hours, so nothing else can have it.
    let received = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--visibility-timeout",
        "43200",
    ])?;
    let handle = as_str(&array(&received, "Messages")?[0]["ReceiptHandle"])?;

    let held = aws.sqs(&["receive-message", "--queue-url", &url])?;
    if held.get("Messages").is_some() {
        return Err("a claimed message was handed to a second consumer".to_owned());
    }

    // Handed back, so claimable at once despite the twelve hours.
    aws.sqs(&[
        "change-message-visibility",
        "--queue-url",
        &url,
        "--receipt-handle",
        &handle,
        "--visibility-timeout",
        "0",
    ])?;

    let again = aws.sqs(&["receive-message", "--queue-url", &url])?;
    expect(&as_str(&array(&again, "Messages")?[0]["Body"])?, "work")
}

fn batch_operations(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-batch")?;

    // One entry is deliberately invalid: the rest must still be sent.
    let entries = json!([
        { "Id": "a", "MessageBody": "one" },
        { "Id": "b", "MessageBody": "two" },
        { "Id": "bad", "MessageBody": "three", "DelaySeconds": 901 },
    ])
    .to_string();

    let sent = aws.sqs(&[
        "send-message-batch",
        "--queue-url",
        &url,
        "--entries",
        &entries,
    ])?;
    if array(&sent, "Successful")?.len() != 2 {
        return Err(format!("expected 2 successful sends, got {sent}"));
    }
    let failed = array(&sent, "Failed")?;
    expect(&as_str(&failed[0]["Id"])?, "bad")?;
    expect(&as_str(&failed[0]["Code"])?, "InvalidParameterValue")?;
    if failed[0]["SenderFault"] != json!(true) {
        return Err("a bad DelaySeconds should be the sender's fault".to_owned());
    }

    // Both good messages really arrived, and both can be deleted in one call.
    let received = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--max-number-of-messages",
        "10",
        "--visibility-timeout",
        "300",
    ])?;
    let messages = array(&received, "Messages")?;
    if messages.len() != 2 {
        return Err(format!("expected 2 messages, got {}", messages.len()));
    }

    let handles: Vec<Value> = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            json!({ "Id": format!("d{index}"), "ReceiptHandle": message["ReceiptHandle"] })
        })
        .collect();
    let deleted = aws.sqs(&[
        "delete-message-batch",
        "--queue-url",
        &url,
        "--entries",
        &Value::Array(handles).to_string(),
    ])?;
    if array(&deleted, "Successful")?.len() != 2 {
        return Err(format!("expected 2 successful deletes, got {deleted}"));
    }

    // And a malformed batch is refused whole.
    let code = aws.sqs_err(&[
        "send-message-batch",
        "--queue-url",
        &url,
        "--entries",
        &json!([
            { "Id": "same", "MessageBody": "x" },
            { "Id": "same", "MessageBody": "y" },
        ])
        .to_string(),
    ])?;
    expect(&code, "BatchEntryIdsNotDistinct")
}

fn queue_attributes(aws: &Aws) -> Result<(), String> {
    let created = aws.sqs(&[
        "create-queue",
        "--queue-name",
        "acc-queue-attrs",
        "--attributes",
        "VisibilityTimeout=120,DelaySeconds=5",
    ])?;
    let url = string(&created, "QueueUrl")?;

    let all = aws.sqs(&[
        "get-queue-attributes",
        "--queue-url",
        &url,
        "--attribute-names",
        "All",
    ])?;
    let attributes = all["Attributes"]
        .as_object()
        .ok_or("no Attributes came back")?;
    expect(&as_str(&attributes["VisibilityTimeout"])?, "120")?;
    expect(&as_str(&attributes["DelaySeconds"])?, "5")?;
    expect(
        &as_str(&attributes["QueueArn"])?,
        "arn:aws:sqs:us-east-1:000000000000:acc-queue-attrs",
    )?;

    // Epoch seconds here, where a message's timestamps are milliseconds.
    let created_at: u64 = as_str(&attributes["CreatedTimestamp"])?
        .parse()
        .map_err(|_| "CreatedTimestamp is not a number".to_owned())?;
    if !(1_000_000_000..10_000_000_000).contains(&created_at) {
        return Err(format!(
            "CreatedTimestamp {created_at} is not epoch seconds"
        ));
    }

    // A partial update leaves what it does not name alone.
    aws.sqs(&[
        "set-queue-attributes",
        "--queue-url",
        &url,
        "--attributes",
        "VisibilityTimeout=600",
    ])?;
    let after = aws.sqs(&[
        "get-queue-attributes",
        "--queue-url",
        &url,
        "--attribute-names",
        "VisibilityTimeout",
        "DelaySeconds",
    ])?;
    expect(&as_str(&after["Attributes"]["VisibilityTimeout"])?, "600")?;
    expect(&as_str(&after["Attributes"]["DelaySeconds"])?, "5")?;

    // The counts split the way they should: one visible, one in flight, one delayed.
    let counting = queue(aws, "acc-counts")?;
    for body in ["one", "two"] {
        aws.sqs(&[
            "send-message",
            "--queue-url",
            &counting,
            "--message-body",
            body,
        ])?;
    }
    aws.sqs(&[
        "send-message",
        "--queue-url",
        &counting,
        "--message-body",
        "later",
        "--delay-seconds",
        "900",
    ])?;
    aws.sqs(&[
        "receive-message",
        "--queue-url",
        &counting,
        "--visibility-timeout",
        "43200",
    ])?;

    let counts = aws.sqs(&[
        "get-queue-attributes",
        "--queue-url",
        &counting,
        "--attribute-names",
        "ApproximateNumberOfMessages",
        "ApproximateNumberOfMessagesNotVisible",
        "ApproximateNumberOfMessagesDelayed",
    ])?;
    let counts = &counts["Attributes"];
    expect(&as_str(&counts["ApproximateNumberOfMessages"])?, "1")?;
    expect(
        &as_str(&counts["ApproximateNumberOfMessagesNotVisible"])?,
        "1",
    )?;
    expect(&as_str(&counts["ApproximateNumberOfMessagesDelayed"])?, "1")
}

fn purge(aws: &Aws) -> Result<(), String> {
    let url = queue(aws, "acc-purge")?;
    for body in ["one", "two"] {
        aws.sqs(&["send-message", "--queue-url", &url, "--message-body", body])?;
    }
    // One in flight, which a purge must take with it.
    let received = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--visibility-timeout",
        "43200",
    ])?;
    let handle = as_str(&array(&received, "Messages")?[0]["ReceiptHandle"])?;

    aws.sqs(&["purge-queue", "--queue-url", &url])?;

    let empty = aws.sqs(&[
        "receive-message",
        "--queue-url",
        &url,
        "--max-number-of-messages",
        "10",
    ])?;
    if empty.get("Messages").is_some() {
        return Err("a purged queue still had messages".to_owned());
    }

    // The in-flight consumer's handle names nothing now.
    let code = aws.sqs_err(&[
        "delete-message",
        "--queue-url",
        &url,
        "--receipt-handle",
        &handle,
    ])?;
    expect(&code, "ReceiptHandleIsInvalid")?;

    // And the queue itself is still usable.
    aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "after",
    ])?;

    Ok(())
}

fn authentication(aws: &Aws) -> Result<(), String> {
    // A working credential is proven by every other check, so this is about the
    // failures, which are the ones that would be easy to get quietly wrong.
    let wrong_secret = aws.with_secret("not-the-secret");
    let code = wrong_secret.sqs_err(&["list-queues"])?;
    expect(&code, "SignatureDoesNotMatch")?;

    // An unknown key id is a different answer from a wrong secret, and neither reveals
    // which half of the credential the caller got wrong.
    let unknown_key = aws.with_key_id(&format!("{KEY_ID}NOPE"));
    let code = unknown_key.sqs_err(&["list-queues"])?;
    expect(&code, "InvalidClientTokenId")
}

fn unimplemented_operations(aws: &Aws) -> Result<(), String> {
    // A real SQS operation that is not built must say so, rather than looking like a
    // typo. All 23 operations are recognised for exactly this reason.
    let url = queue(aws, "acc-unimplemented")?;

    for args in [
        vec!["tag-queue", "--queue-url", &url, "--tags", "Team=platform"],
        vec!["list-queue-tags", "--queue-url", &url],
        vec![
            "add-permission",
            "--queue-url",
            &url,
            "--label",
            "l",
            "--aws-account-ids",
            "000000000000",
            "--actions",
            "SendMessage",
        ],
    ] {
        let code = aws.sqs_err(&args)?;
        if code != "NotImplemented" {
            return Err(format!("aws sqs {} reported {code}", args[0]));
        }
    }

    Ok(())
}

/// The whole produce and consume loop over HTTPS, against a real certificate.
///
/// Its own server, since TLS is a property of the listener rather than of a request. The
/// CLI is told to *trust the authority* rather than to skip verification — a check that
/// passed with verification off would say nothing about whether the chain is right, which
/// is the part that goes wrong in practice.
fn over_tls() -> Result<(), String> {
    let (server, authority) = Server::start_tls()?;
    let aws = server.aws_trusting(&authority);

    if !server.endpoint.starts_with("https://") {
        return Err(format!(
            "expected an https endpoint, got {}",
            server.endpoint
        ));
    }

    // A full round trip, so this covers the handshake, SigV4 over TLS, and the queue
    // URLs the server hands out — which must name https, or a client's next request goes
    // somewhere that is not listening.
    let created = aws.sqs(&["create-queue", "--queue-name", "tls-jobs"])?;
    let url = string(&created, "QueueUrl")?;
    if !url.starts_with("https://") {
        return Err(format!("queue URLs should be https over TLS, got {url}"));
    }

    aws.sqs(&[
        "send-message",
        "--queue-url",
        &url,
        "--message-body",
        "over tls",
    ])?;

    let received = aws.sqs(&["receive-message", "--queue-url", &url])?;
    let message = &array(&received, "Messages")?[0];
    expect(&as_str(&message["Body"])?, "over tls")?;

    aws.sqs(&[
        "delete-message",
        "--queue-url",
        &url,
        "--receipt-handle",
        &as_str(&message["ReceiptHandle"])?,
    ])?;

    // Authentication still applies over TLS: the transport is not the thing that decides
    // who a caller is.
    let code = aws
        .with_secret("not-the-secret")
        .sqs_err(&["list-queues"])?;
    expect(&code, "SignatureDoesNotMatch")?;

    // And a client that does *not* trust the authority must be refused rather than
    // served, which is what proves verification is happening at all.
    let untrusting = server.aws();
    match untrusting.sqs(&["list-queues"]) {
        Ok(output) => Err(format!(
            "a client that trusts nothing should not have been served: {output}"
        )),
        Err(message) => {
            let complained_about_the_certificate = message.contains("SSL")
                || message.contains("certificate")
                || message.contains("CERTIFICATE");

            if complained_about_the_certificate {
                Ok(())
            } else {
                Err(format!(
                    "expected a certificate complaint from an untrusting client, got: \
                     {message}"
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Create a queue and return its URL.
fn queue(aws: &Aws, name: &str) -> Result<String, String> {
    let created = aws.sqs(&["create-queue", "--queue-name", name])?;

    string(&created, "QueueUrl")
}

fn string(value: &Value, field: &str) -> Result<String, String> {
    as_str(&value[field]).map_err(|_| format!("no string {field} in {value}"))
}

fn as_str(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{value} is not a string"))
}

fn array(value: &Value, field: &str) -> Result<Vec<Value>, String> {
    value[field]
        .as_array()
        .cloned()
        .ok_or_else(|| format!("no array {field} in {value}"))
}

fn expect(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }

    Err(format!("expected {expected:?}, got {actual:?}"))
}
