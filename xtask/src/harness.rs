//! Starting a NexQ server and talking to it with the real `aws` CLI.
//!
//! The point of driving the actual CLI rather than a Rust client is that the CLI is not
//! ours: it signs its own requests, validates its own checksums, and shapes its own
//! errors. A test that passes here is evidence about compatibility in a way a test
//! against our own code cannot be.

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The credential in `nexq.example.toml`, which the server is started with.
///
/// No secrets are needed to run this: NexQ is its own trust root, so the acceptance
/// suite works on a fork's pull request exactly as it does on a laptop.
pub const KEY_ID: &str = "AKIANEXQDEV";
pub const SECRET: &str = "change-me";

/// Any string works — SigV4 only needs signer and verifier to agree on one.
pub const REGION: &str = "us-east-1";

/// How long to wait for the server to start listening.
///
/// Generous because a cold CI runner is slow, and the cost of being generous is nothing:
/// the wait ends as soon as the port answers.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// A running server, stopped when this is dropped.
pub struct Server {
    child: Child,
    pub endpoint: String,
}

impl Server {
    /// Build and start `nexq-server` on a free port, waiting until it answers.
    pub fn start() -> Result<Self, String> {
        let binary = build_server()?;
        let port = free_port()?;
        let address = format!("127.0.0.1:{port}");
        let endpoint = format!("http://{address}");

        // The example config is used as-is, so this also checks that the file we tell
        // people to copy actually works.
        let child = Command::new(&binary)
            .env("NEXQ_CONFIG", workspace_root()?.join("nexq.example.toml"))
            .env("NEXQ_AWS_API__BIND_ADDR", &address)
            .env("NEXQ_AWS_API__PUBLIC_BASE_URL", &endpoint)
            .env("RUST_LOG", "nexq=info")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", binary.display()))?;

        let server = Self { child, endpoint };
        server.wait_until_listening(port)?;

        Ok(server)
    }

    /// Poll the port until something answers, or give up.
    fn wait_until_listening(&self, port: u16) -> Result<(), String> {
        let address: SocketAddr = ([127, 0, 0, 1], port).into();
        let deadline = Instant::now() + STARTUP_TIMEOUT;

        while Instant::now() < deadline {
            if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "the server was not listening on {port} within {STARTUP_TIMEOUT:?}"
        ))
    }

    /// An `aws` CLI bound to this server.
    pub fn aws(&self) -> Aws {
        Aws {
            endpoint: self.endpoint.clone(),
            key_id: KEY_ID.to_owned(),
            secret: SECRET.to_owned(),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Killed rather than signalled: graceful shutdown is tested by the unit suite,
        // and a stuck child would hang CI.
        let _ = self.child.kill();
        let _ = self.child.wait();

        // Whatever the server complained about is worth seeing when a check failed.
        if let Some(stderr) = self.child.stderr.take() {
            let lines: Vec<String> = BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
                .filter(|line| line.contains("WARN") || line.contains("ERROR"))
                .collect();

            if !lines.is_empty() {
                eprintln!("\nserver warnings:");
                for line in lines {
                    eprintln!("  {line}");
                }
            }
        }
    }
}

/// The `aws` CLI, pointed at a NexQ server.
///
/// Cloneable so a check can hand one to a thread — long polling needs a second client
/// sending while the first is blocked.
#[derive(Clone)]
pub struct Aws {
    endpoint: String,
    key_id: String,
    secret: String,
}

impl Aws {
    /// The same CLI signing with a different secret, for the auth checks.
    pub fn with_secret(&self, secret: &str) -> Self {
        Self {
            secret: secret.to_owned(),
            ..self.clone()
        }
    }

    /// The same CLI presenting a different access key id.
    pub fn with_key_id(&self, key_id: &str) -> Self {
        Self {
            key_id: key_id.to_owned(),
            ..self.clone()
        }
    }

    /// Run `aws sqs <args>` and parse its JSON output.
    ///
    /// An empty response is `{}` rather than an error: several SQS operations answer
    /// with nothing at all, and the CLI prints nothing for them.
    pub fn sqs(&self, args: &[&str]) -> Result<Value, String> {
        let output = self.run(args)?;

        if output.trim().is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }

        serde_json::from_str(&output)
            .map_err(|error| format!("aws sqs {}: output was not JSON: {error}", args.join(" ")))
    }

    /// Run a command expected to fail, returning the SQS error code the CLI reported.
    ///
    /// The code is what a client actually branches on, so that is what gets asserted
    /// rather than the wording around it.
    pub fn sqs_err(&self, args: &[&str]) -> Result<String, String> {
        match self.run(args) {
            Ok(output) => Err(format!(
                "aws sqs {} should have failed, but printed: {output}",
                args.join(" ")
            )),
            Err(message) => error_code(&message).ok_or_else(|| {
                format!(
                    "aws sqs {} failed without an SQS error code: {message}",
                    args.join(" ")
                )
            }),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("aws")
            .arg("--endpoint-url")
            .arg(&self.endpoint)
            .arg("sqs")
            .args(args)
            .env("AWS_ACCESS_KEY_ID", &self.key_id)
            .env("AWS_SECRET_ACCESS_KEY", &self.secret)
            .env("AWS_DEFAULT_REGION", REGION)
            .env("AWS_DEFAULT_OUTPUT", "json")
            // Nothing should be inherited from a developer's shell or a runner's
            // environment: a stray profile or region would change what is being tested.
            .env_remove("AWS_PROFILE")
            .env_remove("AWS_REGION")
            .env_remove("AWS_ENDPOINT_URL")
            .env_remove("AWS_ENDPOINT_URL_SQS")
            .env_remove("AWS_SESSION_TOKEN")
            // Stops credential resolution from ever reaching for instance metadata,
            // which on a runner costs seconds before it fails.
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .output()
            .map_err(|error| format!("could not run the aws CLI: {error}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }

        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// The SQS error code out of a CLI error message.
///
/// The CLI prints `An error occurred (CodeHere) when calling the X operation: ...`.
fn error_code(message: &str) -> Option<String> {
    let after = message.split_once("An error occurred (")?.1;
    let (code, _) = after.split_once(')')?;

    Some(code.to_owned())
}

/// Build `nexq-server` and return the path to it.
///
/// The path comes from cargo rather than being assembled by hand, so this works with a
/// `CARGO_TARGET_DIR` set, a different profile, or anything else cargo decides.
fn build_server() -> Result<PathBuf, String> {
    // `CARGO` is set for a process cargo launched, so this uses the same cargo that ran
    // the task. Read at runtime rather than baked in at compile time, so a binary built
    // in one place still works when run in another.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let output = Command::new(cargo)
        .args([
            "build",
            "-p",
            "nexq-server",
            "--locked",
            "--message-format",
            "json-render-diagnostics",
        ])
        .current_dir(workspace_root()?)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("could not run cargo build: {error}"))?;

    if !output.status.success() {
        return Err("cargo build -p nexq-server failed".to_owned());
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or_else(|| "cargo build reported no server executable".to_owned())
}

/// A port nothing is listening on.
///
/// Found by binding and releasing, which leaves a moment in which something else could
/// take it. Nothing else is starting servers in a CI job, and a lost race shows up as a
/// clear startup failure rather than as a confusing test result.
fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("could not find a free port: {error}"))?;

    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("could not read the bound port: {error}"))
}

/// The workspace root, from this crate's location rather than the current directory.
fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "could not find the workspace root".to_owned())
}
