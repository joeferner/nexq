//! Build tasks, run as `cargo xtask <task>`:
//!
//! - `acceptance-cli`  — drive a running NexQ server with the real `aws` CLI
//! - `acceptance-node` — drive one with the AWS SDK for JavaScript, a different client
//! - `acceptance-rest`  — drive the REST facade with real `curl`
//! - `openapi`         — write `nexq-api-rest`'s OpenAPI spec to its committed
//!   `openapi.json`, so a change to the published contract shows up in a diff
//! - `openapi-check`   — verify that file matches the code without writing it
//! - `codegen`         — generate the Rust client and the web UI client from that spec
//! - `ui`              — build the embedded SPA
//!
//! Kept in-workspace so an air-gapped build needs no extra tooling, and so a task runs
//! the same way on a laptop as it does in CI — a script only CI runs is a script that
//! rots.

mod acceptance_cli;
mod acceptance_node;
mod acceptance_rest;
mod harness;
mod openapi;

use std::process::ExitCode;

fn main() -> ExitCode {
    let task = std::env::args().nth(1);

    let result = match task.as_deref() {
        Some("acceptance-cli") => acceptance_cli::run(),
        Some("acceptance-node") => acceptance_node::run(),
        Some("acceptance-rest") => acceptance_rest::run(),
        Some("openapi") => openapi::run(false),
        Some("openapi-check") => openapi::run(true),
        Some(unknown) => Err(format!("unknown task {unknown:?}\n\n{USAGE}")),
        None => Err(format!("no task given\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nxtask: {message}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: cargo xtask <task>

tasks:
  acceptance-cli    start a NexQ server and drive it with the real aws CLI
  acceptance-node   the same, with the AWS SDK for JavaScript
  acceptance-rest   drive the REST facade with real curl
  openapi           write nexq-api-rest's OpenAPI document to openapi.json
  openapi-check     verify that committed document matches the code, changing nothing

not built yet:
  codegen           generate the Rust and web UI clients from that spec
  ui                build the embedded SPA";
