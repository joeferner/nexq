//! The generated OpenAPI document.
//!
//! These are the checks that the spec still describes the server, since nothing else will
//! notice if it stops. Every client — the CLI, the SPA, any published SDK — is generated
//! from this document (plan Q18a), so a spec that quietly disagrees with the routing table
//! produces clients that are wrong in a way no compiler catches.

use nexq_api_rest::{API_PREFIX, OPENAPI_PATH};
use nexq_core::engine::{MAX_MESSAGES_PER_RECEIVE, MAX_RECEIVE_WAIT};
use serde_json::Value;

const RECEIVE_PATH: &str = "/api/v1/queues/{queue}/messages/receive";

fn spec() -> Value {
    serde_json::from_str(&nexq_api_rest::openapi_json()).expect("the document must be JSON")
}

/// Every string the document publishes as prose.
fn descriptions(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                if key == "description"
                    && let Some(text) = child.as_str()
                {
                    found.push(text.to_owned());
                }
                descriptions(child, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| descriptions(item, found)),
        _ => {}
    }
}

#[test]
fn the_document_describes_the_route_that_is_actually_served() {
    let spec = spec();

    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(spec["info"]["title"], "NexQ");
    assert_eq!(
        spec["info"]["version"], "0.1.0",
        "the version comes from the crate, so it moves with a release"
    );

    let operation = &spec["paths"][RECEIVE_PATH]["post"];
    assert_eq!(operation["operationId"], "receiveMessages");
    assert!(
        RECEIVE_PATH.starts_with(API_PREFIX),
        "the documented path must carry the prefix the router nests under"
    );

    // The whole documented surface, spelled out. This is the assertion that fails when a
    // route is added without being documented — `route` rather than `api_route`, say — and
    // equally when one is documented that nobody meant to publish.
    let mut paths: Vec<&str> = spec["paths"]
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    paths.sort_unstable();

    assert_eq!(
        paths,
        [
            "/api/v1/queues",
            "/api/v1/queues/{queue}",
            "/api/v1/queues/{queue}/messages",
            "/api/v1/queues/{queue}/messages/delete",
            "/api/v1/queues/{queue}/messages/receive",
            "/api/v1/queues/{queue}/messages/visibility",
            "/api/v1/queues/{queue}/messages/{receipt_handle}",
        ]
    );

    // Every operation carries an id, since that is what names the method on a generated
    // client — an operation without one gets a name invented from its path.
    for (path, operations) in spec["paths"].as_object().expect("an object") {
        for (method, operation) in operations.as_object().expect("an object") {
            assert!(
                operation["operationId"].is_string(),
                "{method} {path} has no operationId"
            );
        }
    }
}

/// `Path<String>` tells aide a parameter exists but not what it is called, and the
/// operation comes out with no parameters at all — which is how this test came to exist.
#[test]
fn the_path_parameter_is_named_and_described() {
    let parameters = spec()["paths"][RECEIVE_PATH]["post"]["parameters"].clone();
    let first = &parameters[0];

    assert_eq!(first["name"], "queue", "got: {parameters}");
    assert_eq!(first["in"], "path");
    assert_eq!(first["required"], true);
    assert!(
        first["description"].as_str().is_some_and(|d| !d.is_empty()),
        "a client generator turns this into a doc comment: {parameters}"
    );
}

/// The handler takes `Option<Json<..>>`, so an empty `POST` is valid. aide infers
/// `required: true` regardless — its own `Option` impl carries a TODO for the body — and
/// `receive_docs` corrects it.
#[test]
fn the_request_body_is_documented_as_optional() {
    let body = spec()["paths"][RECEIVE_PATH]["post"]["requestBody"].clone();

    assert_ne!(
        body["required"], true,
        "an empty body is accepted, so the spec must not demand one: {body}"
    );
    assert!(
        body["content"]["application/json"]["schema"].is_object(),
        "there is still a body schema to send: {body}"
    );
}

/// The bounds are literals in a `schemars` attribute, since an attribute cannot read a
/// constant. This is what keeps them honest.
#[test]
fn the_documented_limits_match_the_engine() {
    let properties = spec()["components"]["schemas"]["ReceiveBody"]["properties"].clone();

    assert_eq!(
        properties["max_messages"]["maximum"].as_u64(),
        Some(MAX_MESSAGES_PER_RECEIVE as u64),
        "the published maximum has drifted from the engine's"
    );
    assert_eq!(properties["max_messages"]["minimum"].as_u64(), Some(1));
    assert_eq!(
        properties["wait_time_seconds"]["maximum"].as_u64(),
        Some(MAX_RECEIVE_WAIT.as_secs()),
        "the published wait cap has drifted from the engine's"
    );
}

/// Authentication is a layer over every route rather than a per-route choice, so the
/// requirement belongs at the top level and not on each operation.
#[test]
fn the_document_says_every_request_needs_a_bearer_token() {
    let spec = spec();

    assert_eq!(
        spec["security"],
        serde_json::json!([{ "bearerAuth": [] }]),
        "got: {}",
        spec["security"]
    );

    let scheme = &spec["components"]["securitySchemes"]["bearerAuth"];
    assert_eq!(scheme["type"], "http");
    assert_eq!(scheme["scheme"], "bearer");
}

#[test]
fn failures_are_documented_with_the_error_envelope() {
    let responses = spec()["paths"][RECEIVE_PATH]["post"]["responses"].clone();

    for status in ["400", "401", "404"] {
        assert_eq!(
            responses[status]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ErrorBody",
            "{status} should carry the one error envelope: {responses}"
        );
    }

    let detail = &spec()["components"]["schemas"]["ErrorDetail"]["properties"];
    assert_eq!(detail["code"]["type"], "string");
    assert_eq!(detail["message"]["type"], "string");
}

/// Every operation in the document.
fn operations(spec: &Value) -> Vec<(String, String, Value)> {
    spec["paths"]
        .as_object()
        .expect("an object")
        .iter()
        .flat_map(|(path, methods)| {
            methods
                .as_object()
                .expect("an object")
                .iter()
                .map(move |(method, operation)| {
                    (method.to_owned(), path.to_owned(), operation.clone())
                })
        })
        .collect()
}

/// A published operation without prose is a client method with no doc comment, which is
/// what most consumers of a generated SDK actually read.
///
/// The two are held to different standards on purpose, and getting that wrong is how this
/// test first failed: a `summary` is a label — "List queues" is a good one — while a
/// `description` is where the behaviour that cannot be inferred from the types goes.
#[test]
fn every_operation_carries_a_summary_and_a_description() {
    for (method, path, operation) in operations(&spec()) {
        let method = method.to_uppercase();

        let summary = operation["summary"].as_str().unwrap_or_default();
        assert!(!summary.is_empty(), "{method} {path} has no summary");
        assert!(
            !summary.contains('\n'),
            "{method} {path}: a summary is a one-line label, not prose: {summary:?}"
        );

        let description = operation["description"].as_str().unwrap_or_default();
        assert!(
            description.len() > 80,
            "{method} {path} needs a description saying what the types cannot: \
             {description:?}"
        );
    }
}

/// Authentication is a layer over *every* route, so every operation can answer `401` —
/// and one that does not say so publishes an endpoint that looks reachable without a
/// token. `error::needs_a_token` is the shared helper; this is what catches forgetting it.
#[test]
fn every_operation_documents_the_401() {
    for (method, path, operation) in operations(&spec()) {
        assert_eq!(
            operation["responses"]["401"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ErrorBody",
            "{} {path} does not document that it needs a token",
            method.to_uppercase()
        );
    }
}

/// The `415` comes from the body extractor rather than from anything an operation decides,
/// so an operation that reads a body must document it — and one that reads none must not.
#[test]
fn an_operation_documents_the_415_exactly_when_it_reads_a_body() {
    for (method, path, operation) in operations(&spec()) {
        let reads_a_body = operation["requestBody"].is_object();
        let documents_415 = operation["responses"]["415"].is_object();

        assert_eq!(
            reads_a_body,
            documents_415,
            "{} {path}: reads a body = {reads_a_body}, documents 415 = {documents_415}",
            method.to_uppercase()
        );
    }
}

/// Every response a client can receive is described, since an undescribed status code in a
/// generated client is a bare number.
#[test]
fn every_documented_response_says_what_it_means() {
    for (method, path, operation) in operations(&spec()) {
        for (status, response) in operation["responses"].as_object().expect("an object") {
            assert!(
                response["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{} {path} response {status} has no description",
                method.to_uppercase()
            );
        }
    }
}

/// A field with no description becomes an undocumented field on a generated type. Not a
/// stylistic preference — the schemas *are* the reference for anyone consuming this API.
#[test]
fn every_schema_field_is_described() {
    let spec = spec();
    let schemas = spec["components"]["schemas"]
        .as_object()
        .expect("an object");

    assert!(!schemas.is_empty(), "there should be schemas");

    for (name, schema) in schemas {
        assert!(
            schema["description"]
                .as_str()
                .is_some_and(|text| !text.is_empty()),
            "schema {name} has no description"
        );

        for (field, property) in schema["properties"].as_object().into_iter().flatten() {
            assert!(
                property["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{name}.{field} has no description"
            );
        }
    }
}

/// Doc comments have two audiences now that `aide` publishes them, and rustdoc's own
/// syntax means nothing to an API consumer — `[`MAX_MESSAGES_PER_RECEIVE`]` reached the
/// spec verbatim before this test existed.
#[test]
fn published_descriptions_are_not_written_for_rustdoc() {
    let mut found = Vec::new();
    descriptions(&spec(), &mut found);

    assert!(!found.is_empty(), "the document should carry descriptions");

    for text in &found {
        assert!(
            !text.contains("[`"),
            "a rustdoc link leaked into the published spec: {text:?}"
        );
    }
}

/// The committed-spec check due next depends on this: two generations must not differ, or
/// the check would fail for reasons that have nothing to do with the API changing.
///
/// Worth knowing what this does *not* prove. `openapi` resets aide's thread-local
/// generation context first, and removing that reset leaves this test green — with one
/// route there is one set of types, so re-extracting them yields identical components. So
/// this pins the property, not the mechanism that will be needed to keep it.
#[test]
fn generating_the_document_twice_gives_the_same_bytes() {
    assert_eq!(
        nexq_api_rest::openapi_json(),
        nexq_api_rest::openapi_json(),
        "generation must be deterministic"
    );
}

#[test]
fn the_document_is_served_from_under_the_api_prefix() {
    assert!(
        OPENAPI_PATH.starts_with(API_PREFIX),
        "{OPENAPI_PATH} should live under {API_PREFIX}"
    );
    assert!(OPENAPI_PATH.ends_with(".json"));
}

/// The committed `openapi.json` beside this crate is what the code generates.
///
/// This is the check that makes the contract reviewable. A route or a type changing is
/// visible in a Rust diff, but what it does to the *published API* is not — and every
/// generated client changes with it. Committing the document turns that into a line in the
/// pull request; this test is what stops the committed copy going stale.
///
/// `make pre-commit` also runs this as its own `openapi-check` step, over the same
/// [`nexq_api_rest::check_openapi`], so the failure is named rather than being one test
/// among hundreds. Kept here as well because `cargo test` is what most people run.
///
/// `include_str!` rather than reading at runtime so cargo treats the file as an input and
/// rebuilds when it changes, and so a missing file is a build failure rather than a test
/// that quietly compares against nothing.
#[test]
fn the_committed_document_is_the_generated_one() {
    const COMMITTED: &str = include_str!("../openapi.json");

    if let Err(report) = nexq_api_rest::check_openapi(COMMITTED) {
        panic!("{report}");
    }
}
