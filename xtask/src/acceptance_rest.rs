//! The REST acceptance suite: real `curl` driving a real NexQ server.
//!
//! The same reasoning as the SQS suite next door. The unit tests drive the router
//! in-process, which proves the handlers behave; this proves the *protocol* — that the
//! paths, methods, status codes and JSON a person would actually send are the ones the
//! server answers to. `curl` is not our code, so a check that passes here is evidence
//! rather than agreement with ourselves.
//!
//! It is also the only place `nexq-server` itself is tested. Everything in
//! `nexq-api-rest` builds a router directly; nothing else checks that the binary reads
//! its config, binds two facades and serves them.
//!
//! Every check gets its own queue, so one failure cannot make the next check lie.
//!
//! Timing is asserted only where timing is the behaviour — a long poll returning early
//! rather than waiting out its deadline. Bounds are loose, because a shared runner is
//! slow and irregular and a flaky acceptance test is worse than a missing one.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::harness::{Answer, Rest, Server};

/// One check: a name, and something to run against a server.
///
/// Every check returns rather than panicking, so a failure reports what it saw and the
/// rest still run — one broken thing should not hide the others.
type Check = (&'static str, fn(&Rest) -> Result<(), String>);

/// Run every check, reporting each and failing if any did not pass.
pub fn run() -> Result<(), String> {
    println!("starting nexq-server and driving its REST facade with curl\n");

    let server = Server::start()?;
    let rest = server.rest();
    println!("  rest at {}\n", server.rest_endpoint);

    let checks: Vec<Check> = vec![
        ("queue lifecycle", queue_lifecycle),
        ("listing and cursor paging", listing_and_paging),
        ("attributes and counts", attributes_and_counts),
        ("send, receive, delete", produce_and_consume),
        ("message attributes", message_attributes),
        ("priority", priority),
        ("position in queue", position_in_queue),
        ("long polling", long_polling),
        ("handing a message back", handing_back),
        ("per-entry results", per_entry_results),
        ("purge", purge),
        ("authentication", authentication),
        ("the error envelope", error_envelope),
        ("the OpenAPI document", openapi_document),
        ("the documentation page", documentation_page),
    ];

    let mut failures = Vec::new();
    for (name, check) in checks {
        match check(&rest) {
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

fn queue_lifecycle(rest: &Rest) -> Result<(), String> {
    let queue = "lifecycle";

    let created = rest.put(&path(queue), &json!({ "delaySeconds": 5 }))?;
    expect_status(&created, 200)?;
    expect(&string(&created.body, "name")?, queue)?;
    expect_number(&created.body["attributes"]["delaySeconds"], 5)?;
    expect_number(&created.body["attributes"]["visibilityTimeoutSeconds"], 30)
        .map_err(|error| format!("an unnamed attribute should take its default: {error}"))?;

    // RFC 3339, which is what a generated client turns into a date type.
    let created_at = string(&created.body, "createdAt")?;
    if !created_at.contains('T') || !created_at.ends_with('Z') {
        return Err(format!("createdAt should be RFC 3339, got {created_at}"));
    }

    // Idempotent: the same request again is the same queue.
    let again = rest.put(&path(queue), &json!({ "delaySeconds": 5 }))?;
    expect_status(&again, 200)?;
    expect(&string(&again.body, "createdAt")?, &created_at)
        .map_err(|error| format!("re-creating should not make a new queue: {error}"))?;

    // Different attributes are a conflict, not a silent reconfiguration.
    let conflict = rest.put(&path(queue), &json!({ "delaySeconds": 6 }))?;
    expect_status(&conflict, 409)?;
    expect_code(&conflict, "queue_already_exists")?;

    let read = rest.get(&path(queue))?;
    expect_status(&read, 200)?;
    expect_number(&read.body["attributes"]["delaySeconds"], 5)?;

    let deleted = rest.delete(&path(queue))?;
    expect_status(&deleted, 204)?;
    if deleted.body != Value::Null {
        return Err(format!("a 204 should carry no body, got {}", deleted.body));
    }

    let gone = rest.get(&path(queue))?;
    expect_status(&gone, 404)?;
    expect_code(&gone, "queue_not_found")
}

/// Paging is by cursor, so churn between pages cannot make a caller skip or repeat one.
fn listing_and_paging(rest: &Rest) -> Result<(), String> {
    let names = ["paging-a", "paging-b", "paging-c", "paging-d"];
    for name in names {
        rest.put(&path(name), &json!({}))?;
    }

    let first = rest.get("/api/v1/queues?prefix=paging-&limit=2")?;
    expect_status(&first, 200)?;
    expect(&page_names(&first.body)?.join(","), "paging-a,paging-b")?;
    let cursor = string(&first.body, "nextCursor")
        .map_err(|_| "more queues remain, so a cursor should have come back".to_owned())?;

    // Delete one from the page already read. An offset of 2 would now skip `paging-c`.
    rest.delete(&path("paging-a"))?;

    let second = rest.get(&format!("/api/v1/queues?prefix=paging-&cursor={cursor}"))?;
    expect_status(&second, 200)?;
    expect(&page_names(&second.body)?.join(","), "paging-c,paging-d")
        .map_err(|error| format!("a cursor should survive churn an offset would not: {error}"))?;

    if second.body["nextCursor"] != Value::Null {
        return Err(format!(
            "the last page should not offer a cursor, got {}",
            second.body["nextCursor"]
        ));
    }

    // A cursor is the server's to issue.
    let refused = rest.get("/api/v1/queues?cursor=not%20a%20name")?;
    expect_status(&refused, 400)?;
    expect_code(&refused, "invalid_cursor")
}

fn attributes_and_counts(rest: &Rest) -> Result<(), String> {
    let queue = "attributes";
    rest.put(
        &path(queue),
        &json!({ "delaySeconds": 5, "visibilityTimeoutSeconds": 120 }),
    )?;

    // A partial update leaves what it does not name alone.
    let patched = rest.patch(&path(queue), &json!({ "delaySeconds": 7 }))?;
    expect_status(&patched, 200)?;
    expect_number(&patched.body["attributes"]["delaySeconds"], 7)?;
    expect_number(&patched.body["attributes"]["visibilityTimeoutSeconds"], 120)
        .map_err(|error| format!("PATCH must not reset an attribute it was not given: {error}"))?;

    // All-or-nothing: a bad attribute alongside a good one changes neither.
    let refused = rest.patch(
        &path(queue),
        &json!({ "delaySeconds": 9, "visibilityTimeoutSeconds": 99_999 }),
    )?;
    expect_status(&refused, 400)?;
    let after = rest.get(&path(queue))?;
    expect_number(&after.body["attributes"]["delaySeconds"], 7)
        .map_err(|error| format!("a refused PATCH must change nothing at all: {error}"))?;

    // Counts are opt-in, and absent otherwise.
    if after.body["counts"] != Value::Null {
        return Err("counts should be absent unless asked for".to_owned());
    }

    rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "one" }, { "body": "two" }] }),
    )?;

    let counted = rest.get(&format!("{}?counts=true", path(queue)))?;
    expect_status(&counted, 200)?;
    expect_number(&counted.body["counts"]["total"], 2)?;
    // Both are delayed, because the queue carries a 7-second delay.
    expect_number(&counted.body["counts"]["delayed"], 2)?;
    expect_number(&counted.body["counts"]["visible"], 0)
}

fn produce_and_consume(rest: &Rest) -> Result<(), String> {
    let queue = "produce";
    rest.put(&path(queue), &json!({}))?;

    let sent = rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "work" }] }),
    )?;
    expect_status(&sent, 200)?;
    let result = &sent.body["results"][0];
    expect(&as_str(&result["status"])?, "accepted")?;
    let id = as_str(&result["messageId"])?;

    let received = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    expect_status(&received, 200)?;
    let message = &received.body["messages"][0];
    expect(&as_str(&message["id"])?, &id)
        .map_err(|error| format!("the message that was sent should come back: {error}"))?;
    expect(&as_str(&message["body"])?, "work")?;
    expect_number(&message["receiveCount"], 1)?;
    let handle = as_str(&message["receiptHandle"])?;

    // Claimed, so nobody else can have it.
    let blocked = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    if !blocked.body["messages"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        return Err(format!(
            "a claimed message should not be handed out twice, got {}",
            blocked.body
        ));
    }

    let deleted = rest.delete(&format!("{}/{handle}", messages(queue)))?;
    expect_status(&deleted, 204)?;

    // A spent handle is refused rather than quietly accepted.
    let stale = rest.delete(&format!("{}/{handle}", messages(queue)))?;
    expect_status(&stale, 400)?;
    expect_code(&stale, "invalid_receipt_handle")
}

fn message_attributes(rest: &Rest) -> Result<(), String> {
    let queue = "attrs";
    rest.put(&path(queue), &json!({}))?;

    rest.post(
        &messages(queue),
        &json!({ "messages": [{
            "body": "hi",
            "attributes": {
                "City":  { "type": "string", "value": "Any City" },
                "Count": { "type": "number", "value": "1250800" },
                "Label": { "type": "string", "label": "uuid", "value": "3f2b1c" },
                "Thumb": { "type": "binary", "value": "aGVsbG8=" },
            },
        }] }),
    )?;

    let received = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    let attributes = &received.body["messages"][0]["attributes"];

    expect(&as_str(&attributes["City"]["type"])?, "string")?;
    expect(&as_str(&attributes["City"]["value"])?, "Any City")?;
    expect(&as_str(&attributes["Count"]["type"])?, "number")?;
    expect(&as_str(&attributes["Label"]["label"])?, "uuid")
        .map_err(|error| format!("a producer's label should survive: {error}"))?;
    expect(&as_str(&attributes["Thumb"]["type"])?, "binary")?;
    expect(&as_str(&attributes["Thumb"]["value"])?, "aGVsbG8=")
        .map_err(|error| format!("binary should come back as the base64 it went in as: {error}"))?;

    // A value that does not match its type is refused, per entry.
    let refused = rest.post(
        &messages(queue),
        &json!({ "messages": [{
            "body": "bad",
            "attributes": { "N": { "type": "number", "value": "banana" } },
        }] }),
    )?;
    expect(&as_str(&refused.body["results"][0]["status"])?, "refused")?;
    expect(
        &as_str(&refused.body["results"][0]["error"]["code"])?,
        "invalid_message_attribute",
    )
}

/// Priority is the reason this API exists — the SQS facade cannot express it.
fn priority(rest: &Rest) -> Result<(), String> {
    let queue = "priority";
    rest.put(&path(queue), &json!({}))?;

    rest.post(
        &messages(queue),
        &json!({ "messages": [
            { "body": "ordinary" },
            { "body": "urgent", "priority": 10 },
        ] }),
    )?;

    let received = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "maxMessages": 2 }),
    )?;
    let first = &received.body["messages"][0];

    expect(&as_str(&first["body"])?, "urgent")
        .map_err(|error| format!("higher priority should be served first: {error}"))?;
    expect_number(&first["priority"], 10)
}

/// "Where am I in line", the other thing the SQS facade cannot answer.
fn position_in_queue(rest: &Rest) -> Result<(), String> {
    let queue = "position";
    rest.put(&path(queue), &json!({}))?;

    let sent = rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "first" }, { "body": "second" }] }),
    )?;
    let first = as_str(&sent.body["results"][0]["messageId"])?;
    let second = as_str(&sent.body["results"][1]["messageId"])?;

    let at = |id: &str| format!("{}/{id}/position", messages(queue));

    let front = rest.get(&at(&first))?;
    expect_status(&front, 200)?;
    expect(&as_str(&front.body["messageId"])?, &first)?;
    expect_number(&front.body["approximatePosition"], 1)
        .map_err(|error| format!("the next message to be served is first in line: {error}"))?;
    expect(&as_str(&front.body["state"])?, "visible")?;

    expect_number(&rest.get(&at(&second))?.body["approximatePosition"], 2)?;

    // A higher-priority arrival moves an existing message backwards, which is why this is
    // reported as approximate rather than as a countdown.
    rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "urgent", "priority": 10 }] }),
    )?;
    expect_number(&rest.get(&at(&first))?.body["approximatePosition"], 2)
        .map_err(|error| format!("an urgent message should have pushed this back: {error}"))?;

    // And a message in flight is out of everyone else's way, since only what is claimable
    // right now is counted.
    let received = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    let urgent = as_str(&received.body["messages"][0]["id"])?;
    expect_number(&rest.get(&at(&first))?.body["approximatePosition"], 1)?;

    let in_flight = rest.get(&at(&urgent))?;
    expect(&as_str(&in_flight.body["state"])?, "notVisible")?;
    expect_number(&in_flight.body["approximatePosition"], 3)
        .map_err(|error| format!("a claimed message is behind everything claimable: {error}"))?;

    // A message the queue does not hold says so as a missing message, not a missing queue.
    let gone = rest.get(&at("never-issued"))?;
    expect_status(&gone, 404)?;
    expect_code(&gone, "message_not_found")
}

/// The request is held open and returns when a message arrives, not when the wait ends.
fn long_polling(rest: &Rest) -> Result<(), String> {
    let queue = "polling";
    rest.put(&path(queue), &json!({}))?;

    let sender = rest.clone();
    let queue_name = queue.to_owned();
    let sending = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        sender.post(
            &messages(&queue_name),
            &json!({ "messages": [{ "body": "late" }] }),
        )
    });

    let started = Instant::now();
    let received = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "waitTimeSeconds": 20 }),
    )?;
    let waited = started.elapsed();

    sending
        .join()
        .map_err(|_| "the sending thread panicked".to_owned())??;

    expect(&as_str(&received.body["messages"][0]["body"])?, "late")?;

    if waited > Duration::from_secs(15) {
        return Err(format!(
            "the send should have woken the poll, but it waited {waited:?} of its 20s"
        ));
    }

    // An empty answer is normal, not an error.
    let empty = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "waitTimeSeconds": 0 }),
    )?;
    expect_status(&empty, 200)?;
    if !empty.body["messages"].as_array().is_some_and(Vec::is_empty) {
        return Err(format!("expected an empty list, got {}", empty.body));
    }

    Ok(())
}

/// A claim re-timed to zero makes the message claimable at once.
fn handing_back(rest: &Rest) -> Result<(), String> {
    let queue = "handback";
    rest.put(&path(queue), &json!({}))?;
    rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "work" }] }),
    )?;

    let received = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    let handle = as_str(&received.body["messages"][0]["receiptHandle"])?;

    let handed_back = rest.patch(
        &format!("{}/{handle}", messages(queue)),
        &json!({ "visibilityTimeoutSeconds": 0 }),
    )?;
    expect_status(&handed_back, 204)?;

    let again = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    expect(&as_str(&again.body["messages"][0]["body"])?, "work")
        .map_err(|error| format!("a handed-back message should be available at once: {error}"))?;
    expect_number(&again.body["messages"][0]["receiveCount"], 2)
}

/// A request carrying several entries is not a transaction.
fn per_entry_results(rest: &Rest) -> Result<(), String> {
    let queue = "entries";
    rest.put(&path(queue), &json!({}))?;

    let sent = rest.post(
        &messages(queue),
        &json!({ "messages": [
            { "body": "fine" },
            { "body": "bad", "delaySeconds": 901 },
            { "body": "also fine" },
        ] }),
    )?;

    expect_status(&sent, 200)
        .map_err(|error| format!("one bad entry must not fail the request: {error}"))?;
    let results = array(&sent.body, "results")?;
    expect(&results.len().to_string(), "3")?;
    expect(&as_str(&results[0]["status"])?, "accepted")?;
    expect(&as_str(&results[1]["status"])?, "refused")?;
    expect_number(&results[1]["index"], 1)?;
    expect(&as_str(&results[2]["status"])?, "accepted")?;

    // Two arrived, not zero and not three.
    let counted = rest.get(&format!("{}?counts=true", path(queue)))?;
    expect_number(&counted.body["counts"]["total"], 2)?;

    // Deleting several at once reports each on its own too.
    let received = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "maxMessages": 2 }),
    )?;
    let handle = as_str(&received.body["messages"][0]["receiptHandle"])?;
    let deleted = rest.post(
        &format!("{}/delete", messages(queue)),
        &json!({ "receiptHandles": [handle, "never-issued"] }),
    )?;
    expect_status(&deleted, 200)?;
    expect(&as_str(&deleted.body["results"][0]["status"])?, "accepted")?;
    expect(&as_str(&deleted.body["results"][1]["status"])?, "refused")?;

    // The two whole-request refusals that survive positional entries.
    let empty = rest.post(&messages(queue), &json!({ "messages": [] }))?;
    expect_status(&empty, 400)?;
    expect_code(&empty, "empty_request")?;

    let many: Vec<Value> = (0..11).map(|n| json!({ "body": n.to_string() })).collect();
    let too_many = rest.post(&messages(queue), &json!({ "messages": many }))?;
    expect_status(&too_many, 400)?;
    expect_code(&too_many, "too_many_entries")
}

/// Purge empties the queue and keeps it, taking claimed messages with it.
fn purge(rest: &Rest) -> Result<(), String> {
    let queue = "purge";
    rest.put(&path(queue), &json!({}))?;
    rest.post(
        &messages(queue),
        &json!({ "messages": [{ "body": "a" }, { "body": "b" }, { "body": "c" }] }),
    )?;

    // Claim one, so the purge has an in-flight message to deal with.
    let received = rest.post(&format!("{}/receive", messages(queue)), &json!({}))?;
    let handle = as_str(&received.body["messages"][0]["receiptHandle"])?;

    let purged = rest.delete(&messages(queue))?;
    expect_status(&purged, 200)?;
    expect_number(&purged.body["purged"], 3)
        .map_err(|error| format!("a purge should take in-flight messages too: {error}"))?;

    let counted = rest.get(&format!("{}?counts=true", path(queue)))?;
    expect_status(&counted, 200)
        .map_err(|error| format!("the queue itself should survive a purge: {error}"))?;
    expect_number(&counted.body["counts"]["total"], 0)?;

    // The handle taken across the purge no longer refers to anything.
    let stale = rest.delete(&format!("{}/{handle}", messages(queue)))?;
    expect_status(&stale, 400)
}

fn authentication(rest: &Rest) -> Result<(), String> {
    let queue = "auth";
    rest.put(&path(queue), &json!({}))?;

    let anonymous = rest.with_token(None).get(&path(queue))?;
    expect_status(&anonymous, 401)?;
    expect_code(&anonymous, "unauthorized")?;

    let wrong_secret = rest
        .with_token(Some("AKIANEXQDEV.wrong"))
        .get(&path(queue))?;
    expect_status(&wrong_secret, 401)?;

    let unknown_key = rest
        .with_token(Some("AKIANOSUCHKEY.change-me"))
        .get(&path(queue))?;
    expect_status(&unknown_key, 401)?;

    // The two must be indistinguishable, so a caller cannot enumerate key ids.
    if wrong_secret.body != unknown_key.body {
        return Err(format!(
            "a wrong secret and an unknown key id should answer identically:\n  {}\n  {}",
            wrong_secret.body, unknown_key.body
        ));
    }

    let malformed = rest.with_token(Some("no-separator")).get(&path(queue))?;
    expect_status(&malformed, 401)
}

/// One envelope for everything, including the failures `axum` would otherwise answer
/// itself in plain text.
fn error_envelope(rest: &Rest) -> Result<(), String> {
    let queue = "envelope";
    rest.put(&path(queue), &json!({}))?;

    let no_route = rest.get("/api/v1/nothing-here")?;
    expect_status(&no_route, 404)?;
    expect_code(&no_route, "no_such_route")?;

    let bad_name = rest.get(&path("bad!name"))?;
    expect_status(&bad_name, 400)?;
    expect_code(&bad_name, "invalid_queue_name")?;

    let out_of_range = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "maxMessages": 99 }),
    )?;
    expect_status(&out_of_range, 400)?;
    expect_code(&out_of_range, "invalid_max_messages")?;

    // A misspelled field is refused rather than ignored.
    let typo = rest.post(
        &format!("{}/receive", messages(queue)),
        &json!({ "visibilityTimeout": 30 }),
    )?;
    expect_status(&typo, 400)?;
    expect_code(&typo, "invalid_request_body")?;

    let bad_query = rest.get("/api/v1/queues?limti=2")?;
    expect_status(&bad_query, 400)?;
    expect_code(&bad_query, "invalid_query_parameter")?;

    // What `curl -d` sends without an explicit content type.
    let form = rest.request_raw(
        "POST",
        &format!("{}/receive", messages(queue)),
        "application/x-www-form-urlencoded",
        "{}",
    )?;
    expect_status(&form, 415)?;
    expect_code(&form, "unsupported_media_type")
}

/// The document is served, and is the one committed to the repository.
fn openapi_document(rest: &Rest) -> Result<(), String> {
    let served = rest.with_token(None).get("/api/v1/openapi.json")?;
    expect_status(&served, 200)
        .map_err(|error| format!("a client generator must be able to fetch this: {error}"))?;

    expect(&as_str(&served.body["openapi"])?, "3.1.0")?;
    expect(&as_str(&served.body["info"]["title"])?, "NexQ")?;

    let committed =
        std::fs::read_to_string(crate::openapi::workspace_root()?.join(crate::openapi::SPEC_FILE))
            .map_err(|error| format!("could not read the committed document: {error}"))?;
    let committed: Value = serde_json::from_str(&committed)
        .map_err(|error| format!("the committed document is not JSON: {error}"))?;

    if served.body != committed {
        return Err(
            "the served document and the committed one differ; run `make openapi`".to_owned(),
        );
    }

    Ok(())
}

/// The documentation page, and the two assets it names, all from this binary.
///
/// The point is that nothing reaches a CDN: the page names only same-origin scripts, and
/// both are actually served. A page that referenced `cdn.jsdelivr.net` would still answer
/// `200` here, which is why the body is read rather than only the status.
fn documentation_page(rest: &Rest) -> Result<(), String> {
    let anonymous = rest.with_token(None);

    let page = anonymous.get("/api/v1/docs")?;
    expect_status(&page, 200)
        .map_err(|error| format!("the docs page should need no token: {error}"))?;

    for named in ["/api/v1/docs/scalar.js", "/api/v1/docs/bootstrap.js"] {
        if !page.text.contains(named) {
            return Err(format!("the page should name {named}:\n{}", page.text));
        }

        let asset = anonymous.get(named)?;
        expect_status(&asset, 200).map_err(|error| format!("{named}: {error}"))?;

        if asset.text.is_empty() {
            return Err(format!("{named} was served empty"));
        }
    }

    if page.text.contains("//cdn.") || page.text.contains("https://unpkg") {
        return Err(format!(
            "the page should load nothing from a CDN:\n{}",
            page.text
        ));
    }

    // The bootstrap turns off the two things the bundle would otherwise send elsewhere.
    let bootstrap = anonymous.get("/api/v1/docs/bootstrap.js")?;
    for setting in ["proxyUrl: ''", "withDefaultFonts: false"] {
        if !bootstrap.text.contains(setting) {
            return Err(format!("the bootstrap should set {setting}"));
        }
    }

    Ok(())
}

/// The same facade over HTTPS, with the client made to trust the authority rather than
/// skip verification — so the chain has to genuinely check out.
fn over_tls() -> Result<(), String> {
    let (server, authority) = Server::start_tls()?;
    let rest = server.rest_trusting(&authority);

    let created = rest.put(&path("tls"), &json!({}))?;
    expect_status(&created, 200)?;
    expect(&string(&created.body, "name")?, "tls")?;

    // A client trusting nothing must be refused rather than served.
    let untrusting = std::process::Command::new("curl")
        .arg("--silent")
        .arg("--show-error")
        .arg(format!("{}/api/v1/queues", server.rest_endpoint))
        .output()
        .map_err(|error| format!("could not run curl: {error}"))?;

    if untrusting.status.success() {
        return Err("a client trusting no authority should not have been served".to_owned());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn path(queue: &str) -> String {
    format!("/api/v1/queues/{queue}")
}

fn messages(queue: &str) -> String {
    format!("/api/v1/queues/{queue}/messages")
}

fn page_names(body: &Value) -> Result<Vec<String>, String> {
    array(body, "queues")?
        .iter()
        .map(|queue| as_str(&queue["name"]))
        .collect()
}

fn string(value: &Value, field: &str) -> Result<String, String> {
    as_str(&value[field]).map_err(|error| format!("{field}: {error}"))
}

fn as_str(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("expected a string, got {value}"))
}

fn array(value: &Value, field: &str) -> Result<Vec<Value>, String> {
    value[field]
        .as_array()
        .cloned()
        .ok_or_else(|| format!("expected {field} to be an array, got {value}"))
}

fn expect(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }

    Err(format!("expected {expected:?}, got {actual:?}"))
}

fn expect_number(value: &Value, expected: u64) -> Result<(), String> {
    match value.as_u64() {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(format!("expected {expected}, got {value}")),
    }
}

fn expect_status(answer: &Answer, expected: u16) -> Result<(), String> {
    if answer.status == expected {
        return Ok(());
    }

    Err(format!(
        "expected HTTP {expected}, got {} with body {}",
        answer.status, answer.body
    ))
}

fn expect_code(answer: &Answer, expected: &str) -> Result<(), String> {
    match answer.code() {
        Some(code) if code == expected => Ok(()),
        Some(code) => Err(format!("expected error code {expected:?}, got {code:?}")),
        None => Err(format!(
            "expected the error envelope with code {expected:?}, got {}",
            answer.body
        )),
    }
}
