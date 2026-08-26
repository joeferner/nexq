//! Starting a NexQ server and talking to it with the real `aws` CLI.
//!
//! The point of driving the actual CLI rather than a Rust client is that the CLI is not
//! ours: it signs its own requests, validates its own checksums, and shapes its own
//! errors. A test that passes here is evidence about compatibility in a way a test
//! against our own code cannot be.

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
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
        Self::start_with(None)
    }

    /// The same, serving HTTPS with a generated certificate.
    ///
    /// `authority` comes back so a client can be told to trust it — the point of testing
    /// TLS with a real certificate rather than by skipping verification is that the chain
    /// has to actually check out.
    pub fn start_tls() -> Result<(Self, PathBuf), String> {
        let chain = TestChain::generate()?;
        let authority = chain.authority.clone();

        Ok((Self::start_with(Some(chain))?, authority))
    }

    fn start_with(tls: Option<TestChain>) -> Result<Self, String> {
        let binary = build_server()?;

        // Every facade the example config enables gets a port of its own, not just the
        // one being driven: a facade left on its default port makes a second server
        // impossible to start while the first is running, which is exactly what the TLS
        // check does.
        let [port, rest_port] = free_ports()?;
        let address = format!("127.0.0.1:{port}");
        let scheme = if tls.is_some() { "https" } else { "http" };
        let endpoint = format!("{scheme}://localhost:{port}");

        // The example config is used as-is, so this also checks that the file we tell
        // people to copy actually works.
        let mut command = Command::new(&binary);
        command
            .env("NEXQ_CONFIG", workspace_root()?.join("nexq.example.toml"))
            .env("NEXQ_AWS_API__BIND_ADDR", &address)
            .env("NEXQ_AWS_API__PUBLIC_BASE_URL", &endpoint)
            .env("NEXQ_REST_API__BIND_ADDR", format!("127.0.0.1:{rest_port}"))
            .env("RUST_LOG", "nexq=info")
            // Both, because they carry different things: the log goes to stdout, and a
            // panic goes to stderr. Read only when something has gone wrong, which is
            // safe because nothing here logs per request — a server that chattered on
            // every request could fill a pipe nobody is draining and block.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(chain) = &tls {
            // Set through the environment rather than a config file, which also exercises
            // the `NEXQ_*__*` override path for a nested table.
            command
                .env("NEXQ_AWS_API__TLS__CERTIFICATE", &chain.certificate)
                .env("NEXQ_AWS_API__TLS__PRIVATE_KEY", &chain.private_key);
        }

        let child = command
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", binary.display()))?;

        let mut server = Self { child, endpoint };
        server.wait_until_listening(port)?;

        Ok(server)
    }

    /// Poll the port until something answers, or give up.
    fn wait_until_listening(&mut self, port: u16) -> Result<(), String> {
        let address: SocketAddr = ([127, 0, 0, 1], port).into();
        let deadline = Instant::now() + STARTUP_TIMEOUT;

        while Instant::now() < deadline {
            if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                return Ok(());
            }

            // A server that has already exited will never answer, and it said why on the
            // way out. Waiting out the timeout would report the symptom and hide the
            // cause — a bad certificate or a taken port looks the same from the socket.
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!(
                    "the server exited before it was listening on {port} ({status}):{}",
                    indented(&self.output_lines())
                ));
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "the server was not listening on {port} within {STARTUP_TIMEOUT:?}"
        ))
    }

    /// Everything the server printed, its log and any panic together.
    ///
    /// The pipes are taken rather than borrowed, so reading cannot block on a process
    /// that is still running and still writing: this is only called once the server has
    /// stopped, and whichever of startup and drop asks first gets the output.
    fn output_lines(&mut self) -> Vec<String> {
        let stdout = self.child.stdout.take().map(read_lines).unwrap_or_default();
        let stderr = self.child.stderr.take().map(read_lines).unwrap_or_default();

        [stdout, stderr].concat()
    }

    /// An `aws` CLI bound to this server.
    pub fn aws(&self) -> Aws {
        Aws {
            endpoint: self.endpoint.clone(),
            key_id: KEY_ID.to_owned(),
            secret: SECRET.to_owned(),
            ca_bundle: None,
        }
    }

    /// The same, told to trust a certificate authority.
    pub fn aws_trusting(&self, authority: &Path) -> Aws {
        Aws {
            ca_bundle: Some(authority.to_path_buf()),
            ..self.aws()
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
        let lines: Vec<String> = self
            .output_lines()
            .into_iter()
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

fn read_lines(stream: impl std::io::Read) -> Vec<String> {
    BufReader::new(stream)
        .lines()
        .map_while(Result::ok)
        .collect()
}

/// Server output, laid out under the message that is reporting it.
///
/// Only the tail, since the startup logs above a failure are noise, and indented to the
/// width a failing check's reason is printed at.
fn indented(lines: &[String]) -> String {
    const CONTEXT: usize = 10;

    lines
        .iter()
        .skip(lines.len().saturating_sub(CONTEXT))
        .map(|line| format!("\n          {line}"))
        .collect()
}

/// A certificate authority and a `localhost` certificate it signed.
///
/// Two certificates rather than one self-signed, because a certificate marked as a CA
/// cannot also be an end entity — a client refuses it — and because a chain is the
/// realistic shape: the client trusts the authority, the server presents the leaf.
struct TestChain {
    authority: PathBuf,
    certificate: PathBuf,
    private_key: PathBuf,
}

impl TestChain {
    /// Generated per run with `openssl` rather than committed, so nothing starts failing
    /// on the day a checked-in certificate would have expired.
    fn generate() -> Result<Self, String> {
        let directory = std::env::temp_dir().join("nexq-acceptance-tls");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("could not create {}: {error}", directory.display()))?;

        let path = |file: &str| directory.join(file);
        let text = |file: &str| path(file).display().to_string();

        openssl(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=NexQ Acceptance CA",
            "-keyout",
            &text("ca.key"),
            "-out",
            &text("ca.pem"),
        ])?;

        openssl(&[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-keyout",
            &text("server.key"),
            "-out",
            &text("server.csr"),
        ])?;

        // `localhost` in a SAN, since that is the host the CLI will be told to reach and
        // where a modern client looks for the name.
        std::fs::write(
            path("server.ext"),
            "subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\n",
        )
        .map_err(|error| format!("could not write extensions: {error}"))?;

        openssl(&[
            "x509",
            "-req",
            "-days",
            "1",
            "-in",
            &text("server.csr"),
            "-CA",
            &text("ca.pem"),
            "-CAkey",
            &text("ca.key"),
            "-extfile",
            &text("server.ext"),
            "-out",
            &text("server.pem"),
        ])?;

        Ok(Self {
            authority: path("ca.pem"),
            certificate: path("server.pem"),
            private_key: path("server.key"),
        })
    }
}

fn openssl(arguments: &[&str]) -> Result<(), String> {
    let output = Command::new("openssl")
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run openssl: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    Err(format!(
        "openssl {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
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

    /// A certificate authority for the CLI to trust, when the server is serving HTTPS.
    ca_bundle: Option<PathBuf>,
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
        let mut command = Command::new("aws");
        command.arg("--endpoint-url").arg(&self.endpoint);

        // The CLI is told to trust the generated authority rather than to skip
        // verification, so the chain has to actually check out for a check to pass.
        if let Some(ca_bundle) = &self.ca_bundle {
            command.arg("--ca-bundle").arg(ca_bundle);
        }

        let output = command
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

/// `N` distinct ports nothing is listening on.
///
/// Found by binding and releasing, which leaves a moment in which something else could
/// take one. Nothing else is starting servers in a CI job, and a lost race shows up as a
/// clear startup failure rather than as a confusing test result.
///
/// All of them are held at once before any is released, since binding and releasing one
/// at a time can hand back the same port twice.
fn free_ports<const N: usize>() -> Result<[u16; N], String> {
    let mut listeners = Vec::with_capacity(N);
    let mut ports = [0; N];

    for port in &mut ports {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("could not find a free port: {error}"))?;

        *port = listener
            .local_addr()
            .map_err(|error| format!("could not read the bound port: {error}"))?
            .port();

        listeners.push(listener);
    }

    Ok(ports)
}

/// The workspace root, from this crate's location rather than the current directory.
fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "could not find the workspace root".to_owned())
}
