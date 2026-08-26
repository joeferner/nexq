//! Build tasks, run as `cargo xtask <task>`:
//!
//! - `acceptance-cli`  — drive a running NexQ server with the real `aws` CLI
//! - `acceptance-node` — drive one with the AWS SDK for JavaScript, a different client
//! - `openapi`         — dump the OpenAPI spec from `nexq-api-rest`'s router
//! - `codegen`         — generate the Rust client and the web UI client from that spec
//! - `ui`              — build the embedded SPA
//!
//! Kept in-workspace so an air-gapped build needs no extra tooling, and so a task runs
//! the same way on a laptop as it does in CI — a script only CI runs is a script that
//! rots.

mod acceptance_cli;
mod acceptance_node;
mod harness;

use std::process::ExitCode;

fn main() -> ExitCode {
    let task = std::env::args().nth(1);

    let result = match task.as_deref() {
        Some("acceptance-cli") => acceptance_cli::run(),
        Some("acceptance-node") => acceptance_node::run(),
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

not built yet:
  openapi           dump the OpenAPI spec from nexq-api-rest's router
  codegen           generate the Rust and web UI clients from that spec
  ui                build the embedded SPA";
