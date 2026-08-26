//! The Node acceptance suite: the AWS SDK for JavaScript driving a real NexQ server.
//!
//! A second client on purpose. The `aws` CLI is botocore, so passing only against it
//! leaves NexQ compatible with one implementation of SQS's protocol rather than with the
//! protocol — and this SDK differs where it matters: its own SigV4 signer, its own error
//! deserialiser, its own paginator, and an MD5 validator botocore does not have.
//!
//! The checks live in `acceptance/node/`, since they have to be written against the SDK.
//! This starts the server, installs the SDK if needed, and reports what the suite said.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::harness::{KEY_ID, REGION, SECRET, Server};

/// Start a server and run the Node suite against it.
pub fn run() -> Result<(), String> {
    let directory = suite_directory()?;

    println!("starting nexq-server and driving it with the AWS SDK for JavaScript\n");

    install(&directory)?;

    let server = Server::start()?;
    println!("  server at {}\n", server.endpoint);

    let status = Command::new("node")
        .arg("acceptance.mjs")
        .current_dir(&directory)
        .env("NEXQ_ENDPOINT", &server.endpoint)
        .env("AWS_ACCESS_KEY_ID", KEY_ID)
        .env("AWS_SECRET_ACCESS_KEY", SECRET)
        .env("AWS_DEFAULT_REGION", REGION)
        // As with the CLI suite: nothing should be inherited that could change what is
        // being tested.
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_REGION")
        .env_remove("AWS_ENDPOINT_URL")
        .env_remove("AWS_ENDPOINT_URL_SQS")
        .env_remove("AWS_SESSION_TOKEN")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .status()
        .map_err(|error| {
            format!("could not run node: {error}. Is Node.js installed and on the path?")
        })?;

    if status.success() {
        return Ok(());
    }

    Err("the Node acceptance suite failed".to_owned())
}

/// Install the SDK, preferring the lockfile so a run is reproducible.
///
/// Skipped when `node_modules` is already there, which keeps a second local run instant.
/// CI starts clean, so CI always installs.
fn install(directory: &Path) -> Result<(), String> {
    if directory.join("node_modules").is_dir() {
        return Ok(());
    }

    // `npm ci` installs exactly the lockfile and fails if it disagrees with the
    // manifest, which is what makes the run reproducible rather than "whatever npm
    // resolved today".
    let command = if directory.join("package-lock.json").is_file() {
        "ci"
    } else {
        "install"
    };

    println!("  installing the SDK with npm {command}");
    let status = Command::new("npm")
        .args([command, "--no-audit", "--no-fund"])
        .current_dir(directory)
        .status()
        .map_err(|error| {
            format!("could not run npm: {error}. Is Node.js installed and on the path?")
        })?;

    if status.success() {
        return Ok(());
    }

    Err(format!("npm {command} failed"))
}

fn suite_directory() -> Result<PathBuf, String> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "could not find the workspace root".to_owned())?
        .join("acceptance")
        .join("node");

    if !directory.is_dir() {
        return Err(format!("{} is missing", directory.display()));
    }

    Ok(directory)
}
