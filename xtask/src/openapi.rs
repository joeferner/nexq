//! `cargo xtask openapi` — write the OpenAPI document to the file every client is
//! generated from.
//!
//! The document is generated from `nexq-api-rest`'s routing table, so this task is a dump
//! rather than a build: it links the facade in and asks it. There is no template to keep in
//! step and no way for this to describe a route the server does not serve.
//!
//! The committed file exists so the contract is **reviewable**. A change to a route or a
//! type is otherwise invisible in a diff — the Rust change is there, but what it does to
//! the published API is not, and every generated client changes with it silently. Checking
//! the file in makes that a line in the pull request.

use std::path::PathBuf;

/// The committed document, relative to the workspace root.
///
/// Beside the crate that generates it rather than at the workspace root: it belongs to the
/// REST facade, and a generated artifact at the top of the tree reads like something you
/// are meant to edit.
///
/// Read from the other side by `nexq-api-rest`'s
/// `the_committed_document_is_the_generated_one` test; if the two ever disagree about the
/// path, that test compares against a file this task does not write and fails.
pub const SPEC_FILE: &str = "crates/nexq-api-rest/openapi.json";

/// Write the document. `check` verifies instead, changing nothing and failing on a
/// difference — which is what `make pre-commit` and CI run.
pub fn run(check: bool) -> Result<(), String> {
    let path = workspace_root()?.join(SPEC_FILE);
    let generated = nexq_api_rest::openapi_json();

    if check {
        // Read rather than `include_str!`: a missing file must fail with an explanation of
        // how to create it, not refuse to compile.
        let committed = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "could not read {}: {error}\n\nrun `cargo xtask openapi` to create it",
                path.display()
            )
        })?;

        // The same comparison the test uses, so the two cannot explain a difference
        // differently.
        nexq_api_rest::check_openapi(&committed)?;

        println!("{SPEC_FILE} matches the code ({} bytes)", generated.len());
        return Ok(());
    }

    let previous = std::fs::read_to_string(&path).unwrap_or_default();
    if previous == generated {
        println!(
            "{SPEC_FILE} is already up to date ({} bytes)",
            generated.len()
        );
        return Ok(());
    }

    std::fs::write(&path, &generated)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    println!(
        "wrote {SPEC_FILE} ({} bytes){}",
        generated.len(),
        if previous.is_empty() {
            String::new()
        } else {
            format!(", was {} bytes", previous.len())
        }
    );
    println!("review the diff: this is a change to the published contract");

    Ok(())
}

/// The workspace root, from this crate's location rather than the current directory.
///
/// Its own copy rather than `harness`'s, so this task does not pull in a module that
/// builds and starts servers.
fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "could not find the workspace root".to_owned())
}
