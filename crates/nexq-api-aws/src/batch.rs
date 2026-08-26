//! What the three batch operations have in common.
//!
//! Batching in SQS is a wire-protocol convenience, not a transaction. Each entry
//! succeeds or fails on its own, and the response carries both outcomes side by side —
//! so a batch of ten with one bad receipt handle answers `200` with nine `Successful`
//! entries and one `Failed`. A client that treats a batch as all-or-nothing has
//! misread the API, and a server that made it so would be the one at fault.
//!
//! That shape is why nothing here reaches the engine: the batch operations decompose
//! into the single-message ones the engine already has, and each entry runs through the
//! same code a lone request would. Doing it in one storage round trip is an
//! optimisation for a backend that can, not a change in meaning.
//!
//! # What fails the whole batch
//!
//! Five things, and they are all about the *list* rather than its contents — an empty
//! list, too many entries, a duplicate id, a malformed id, or more bytes than one
//! message may hold. There is no per-entry answer to give when the request itself does
//! not parse as a batch, and a duplicate id is the clearest case: results are matched
//! back by id, so two entries sharing one leaves no answer that means anything.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::error::ApiError;

/// Most entries SQS accepts in one batch.
pub const MAX_ENTRIES: usize = 10;

/// Longest a batch entry `Id` may be.
pub const MAX_ID_LEN: usize = 80;

/// One entry of a batch: the id results are reported against, and its own input.
#[derive(Debug)]
pub struct Entry {
    /// The client's label for this entry. Echoed back in the result, and the only way a
    /// client can tell which entry an outcome belongs to.
    pub id: String,

    /// The entry's fields, shaped like the single-message operation's input, minus the
    /// `QueueUrl` that the batch carries once for all of them.
    pub input: Map<String, Value>,
}

/// Read and check the `Entries` of a batch request.
///
/// Everything checked here is fatal to the batch. Anything that could sensibly be
/// reported against one entry is left for the caller to discover while running it.
pub fn entries(input: &Map<String, Value>) -> Result<Vec<Entry>, ApiError> {
    let listed = match input.get("Entries") {
        // Absent and empty are the same complaint: SQS has a named error for it rather
        // than treating a batch of nothing as a batch that trivially succeeded.
        None | Some(Value::Null) => return Err(ApiError::empty_batch_request()),
        Some(Value::Array(listed)) => listed,
        Some(_) => {
            return Err(ApiError::invalid_parameter_value(
                "Entries must be a list of batch entries.",
            ));
        }
    };

    if listed.is_empty() {
        return Err(ApiError::empty_batch_request());
    }

    if listed.len() > MAX_ENTRIES {
        return Err(ApiError::too_many_entries_in_batch_request(
            listed.len(),
            MAX_ENTRIES,
        ));
    }

    let mut entries = Vec::with_capacity(listed.len());
    let mut seen = HashSet::with_capacity(listed.len());

    for value in listed {
        let Value::Object(fields) = value else {
            return Err(ApiError::invalid_parameter_value(
                "Each batch entry must be an object.",
            ));
        };

        let id = entry_id(fields)?;
        if !seen.insert(id.clone()) {
            return Err(ApiError::batch_entry_ids_not_distinct(&id));
        }

        entries.push(Entry {
            id,
            input: fields.clone(),
        });
    }

    Ok(entries)
}

/// Render a batch response from what each entry did.
///
/// Both lists are omitted when empty, the way SQS omits `Messages` and `QueueUrls`:
/// a batch where everything worked has no `Failed` key at all, so a client checking for
/// its presence gets the answer it expects.
///
/// `success` shapes the successful half, since only `SendMessageBatch` has anything to
/// say beyond the id.
pub fn results(outcomes: Vec<(String, Result<Value, ApiError>)>) -> Value {
    let mut successful = Vec::new();
    let mut failed = Vec::new();

    for (id, outcome) in outcomes {
        match outcome {
            Ok(Value::Object(mut fields)) => {
                fields.insert("Id".to_owned(), json!(id));
                successful.push(Value::Object(fields));
            }
            // A success with nothing to report, which is `DeleteMessageBatch` and
            // `ChangeMessageVisibilityBatch`: the id alone says it worked.
            Ok(_) => successful.push(json!({ "Id": id })),
            Err(error) => failed.push(json!({
                "Id": id,
                "Code": error.code(),
                "Message": error.message(),
                // Whose fault it was, which is how a client decides whether retrying
                // could possibly help.
                "SenderFault": error.is_sender_fault(),
            })),
        }
    }

    let mut output = Map::new();
    if !successful.is_empty() {
        output.insert("Successful".to_owned(), Value::Array(successful));
    }
    if !failed.is_empty() {
        output.insert("Failed".to_owned(), Value::Array(failed));
    }

    Value::Object(output)
}

/// Read and check one entry's `Id`.
///
/// The rules are SQS's: up to [`MAX_ID_LEN`] characters, each alphanumeric, `-`, or `_`.
fn entry_id(fields: &Map<String, Value>) -> Result<String, ApiError> {
    let Some(Value::String(id)) = fields.get("Id") else {
        return Err(ApiError::invalid_batch_entry_id(
            "A batch entry id is required.",
        ));
    };

    if id.is_empty() {
        return Err(ApiError::invalid_batch_entry_id(
            "A batch entry id must not be empty.",
        ));
    }

    if id.chars().count() > MAX_ID_LEN {
        return Err(ApiError::invalid_batch_entry_id(format!(
            "A batch entry id can be at most {MAX_ID_LEN} characters long."
        )));
    }

    if let Some(character) = id
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
    {
        return Err(ApiError::invalid_batch_entry_id(format!(
            "A batch entry id can only contain alphanumeric characters, hyphens, and \
             underscores, but {id:?} contains {character:?}."
        )));
    }

    Ok(id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: Value) -> Result<Vec<Entry>, ApiError> {
        let Value::Object(input) = input else {
            panic!("test input must be an object");
        };

        entries(&input)
    }

    fn ids(input: Value) -> Vec<String> {
        parsed(input)
            .expect("valid entries")
            .into_iter()
            .map(|entry| entry.id)
            .collect()
    }

    #[test]
    fn entries_keep_their_ids_and_their_fields() {
        let entries = parsed(json!({
            "Entries": [
                { "Id": "a", "MessageBody": "one" },
                { "Id": "b", "MessageBody": "two" },
            ]
        }))
        .expect("valid");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "a");
        assert_eq!(entries[0].input["MessageBody"], "one");
        assert_eq!(entries[1].id, "b");
    }

    #[test]
    fn the_order_entries_were_sent_in_is_kept() {
        // Results are matched by id, so order is not load-bearing — but reordering would
        // change which message reaches a queue first, which callers do notice.
        assert_eq!(
            ids(json!({ "Entries": [
                { "Id": "c" }, { "Id": "a" }, { "Id": "b" },
            ] })),
            ["c", "a", "b"]
        );
    }

    #[test]
    fn a_batch_with_nothing_in_it_is_refused() {
        for input in [
            json!({}),
            json!({ "Entries": null }),
            json!({ "Entries": [] }),
        ] {
            let error = parsed(input.clone()).expect_err(&input.to_string());

            assert_eq!(error.code(), "EmptyBatchRequest", "{input}");
        }
    }

    #[test]
    fn a_batch_larger_than_sqs_allows_is_refused() {
        let entries: Vec<Value> = (0..=MAX_ENTRIES)
            .map(|index| json!({ "Id": format!("e{index}") }))
            .collect();

        let error = parsed(json!({ "Entries": entries })).expect_err("eleven");

        assert_eq!(error.code(), "TooManyEntriesInBatchRequest");
        assert!(error.message().contains("10"), "{}", error.message());
    }

    #[test]
    fn exactly_the_maximum_is_allowed() {
        let entries: Vec<Value> = (0..MAX_ENTRIES)
            .map(|index| json!({ "Id": format!("e{index}") }))
            .collect();

        assert_eq!(ids(json!({ "Entries": entries })).len(), MAX_ENTRIES);
    }

    #[test]
    fn a_repeated_id_fails_the_whole_batch() {
        // Not one entry: results are matched back by id, so a duplicate leaves no answer
        // that means anything for either of them.
        let error = parsed(json!({ "Entries": [
            { "Id": "a" }, { "Id": "b" }, { "Id": "a" },
        ] }))
        .expect_err("repeated");

        assert_eq!(error.code(), "BatchEntryIdsNotDistinct");
        assert!(error.message().contains('a'), "{}", error.message());
    }

    #[test]
    fn ids_differing_only_by_case_are_distinct() {
        // Ids are compared as sent, so `A` and `a` are two entries. Folding case would
        // reject a batch SQS accepts.
        assert_eq!(
            ids(json!({ "Entries": [{ "Id": "a" }, { "Id": "A" }] })),
            ["a", "A"]
        );
    }

    #[test]
    fn an_id_must_follow_the_rules_for_one() {
        for (id, why) in [
            (json!(""), "empty"),
            (json!("with space"), "space"),
            (json!("with.dot"), "period"),
            (json!("emoji🎉"), "not ascii"),
            (json!("n".repeat(MAX_ID_LEN + 1)), "too long"),
            (json!(7), "not a string"),
            (json!(null), "null"),
        ] {
            let error =
                parsed(json!({ "Entries": [{ "Id": id }] })).expect_err(&format!("{id}: {why}"));

            assert_eq!(error.code(), "InvalidBatchEntryId", "{id}: {why}");
        }

        let error =
            parsed(json!({ "Entries": [{ "MessageBody": "no id" }] })).expect_err("no id at all");
        assert_eq!(error.code(), "InvalidBatchEntryId");
    }

    #[test]
    fn the_ids_sqs_allows_are_allowed() {
        for id in ["a", "A1", "with-hyphen", "with_underscore", "0"] {
            parsed(json!({ "Entries": [{ "Id": id }] }))
                .unwrap_or_else(|error| panic!("{id:?}: {error:?}"));
        }

        let longest = "n".repeat(MAX_ID_LEN);
        parsed(json!({ "Entries": [{ "Id": longest }] })).expect("exactly at the limit");
    }

    #[test]
    fn entries_that_are_not_objects_are_refused() {
        for input in [
            json!({ "Entries": "a,b" }),
            json!({ "Entries": ["a"] }),
            json!({ "Entries": [7] }),
        ] {
            let error = parsed(input.clone()).expect_err(&input.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{input}");
        }
    }

    #[test]
    fn a_batch_where_everything_worked_has_no_failed_list() {
        let output = results(vec![
            ("a".to_owned(), Ok(json!({ "MessageId": "1" }))),
            ("b".to_owned(), Ok(json!({}))),
        ]);

        let successful = output["Successful"].as_array().expect("successful");
        assert_eq!(successful.len(), 2);
        assert_eq!(successful[0]["Id"], "a");
        assert_eq!(successful[0]["MessageId"], "1");
        assert_eq!(successful[1]["Id"], "b");
        assert!(
            output.get("Failed").is_none(),
            "an empty list would make the CLI print one: {output}"
        );
    }

    #[test]
    fn a_batch_where_nothing_worked_has_no_successful_list() {
        let output = results(vec![(
            "a".to_owned(),
            Err(ApiError::receipt_handle_is_invalid()),
        )]);

        assert!(output.get("Successful").is_none(), "{output}");
        assert_eq!(output["Failed"][0]["Id"], "a");
    }

    #[test]
    fn successes_and_failures_come_back_together() {
        // The whole point of a batch: one bad entry does not sink the good ones.
        let output = results(vec![
            ("good".to_owned(), Ok(json!({ "MessageId": "1" }))),
            ("bad".to_owned(), Err(ApiError::receipt_handle_is_invalid())),
            ("also-good".to_owned(), Ok(json!({ "MessageId": "2" }))),
        ]);

        assert_eq!(output["Successful"].as_array().expect("ok").len(), 2);
        assert_eq!(output["Failed"].as_array().expect("failed").len(), 1);
        assert_eq!(output["Failed"][0]["Id"], "bad");
        assert_eq!(output["Failed"][0]["Code"], "ReceiptHandleIsInvalid");
    }

    #[test]
    fn a_failure_says_whose_fault_it_was() {
        // How a client decides whether retrying could possibly help: its own bad input
        // will fail again, a server problem might not.
        let output = results(vec![
            (
                "mine".to_owned(),
                Err(ApiError::receipt_handle_is_invalid()),
            ),
            ("theirs".to_owned(), Err(ApiError::internal_error())),
        ]);

        let failed = output["Failed"].as_array().expect("failed");
        assert_eq!(
            failed[0]["SenderFault"], true,
            "a 4xx is the caller's doing"
        );
        assert_eq!(
            failed[1]["SenderFault"], false,
            "a 5xx is this server's, and worth retrying"
        );
    }

    #[test]
    fn a_failure_carries_the_code_and_message_the_single_operation_would_have() {
        let error = ApiError::queue_does_not_exist();
        let expected_message = error.message().to_owned();

        let output = results(vec![("a".to_owned(), Err(error))]);

        assert_eq!(output["Failed"][0]["Code"], "QueueDoesNotExist");
        assert_eq!(output["Failed"][0]["Message"], expected_message);
    }
}
