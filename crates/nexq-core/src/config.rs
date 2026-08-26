//! Configuration: a TOML file with environment-variable overrides.
//!
//! Layers, lowest precedence first:
//!
//! 1. defaults compiled in here
//! 2. a TOML file — `nexq.toml` in the working directory, or the path in `NEXQ_CONFIG`
//! 3. environment variables prefixed `NEXQ_`, with `__` separating nested keys
//!
//! So `aws_api.bind_addr` is `NEXQ_AWS_API__BIND_ADDR`. A missing config file is not an
//! error; a config with no credentials is, since every request must be authenticated.
//!
//! Each protocol facade owns its own listener, so each has its own section. Credentials
//! are shared across all of them: one registry of principals, presented differently by
//! each facade — see [`Credential`].
//!
//! ```toml
//! [[auth.credentials]]
//! name = "dev"
//! key_id = "AKIANEXQEXAMPLE"
//! secret = "shhh"
//!
//! [aws_api]
//! bind_addr = "0.0.0.0:8080"
//! public_base_url = "http://localhost:8080"
//! ```
//!
//! ```no_run
//! let config = nexq_core::Config::load()?;
//! # Ok::<(), figment::Error>(())
//! ```

// `figment::Error` is 208 bytes, over clippy's 128-byte threshold. Boxing it would
// only push the cost onto every caller's error handling for a type that is returned
// once at startup, and `figment::Jail` fixes the error type in test closures anyway.
#![allow(clippy::result_large_err)]

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::time::Duration;

use figment::providers::{Env, Format, Toml};
use figment::{Figment, Metadata, Profile, Provider};
use serde::{Deserialize, Serialize};

/// Environment variable naming the config file to read.
pub const CONFIG_PATH_ENV: &str = "NEXQ_CONFIG";

/// Config file read when [`CONFIG_PATH_ENV`] is unset.
pub const DEFAULT_CONFIG_FILE: &str = "nexq.toml";

/// Prefix for environment-variable overrides.
pub const ENV_PREFIX: &str = "NEXQ_";

/// Separator for nested keys in environment variables: `NEXQ_AWS_API__BIND_ADDR`.
pub const ENV_NESTED_SEPARATOR: &str = "__";

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Credentials, shared by every facade.
    pub auth: AuthConfig,

    /// The SQS/SNS-compatible facade.
    #[serde(default)]
    pub aws_api: AwsApiConfig,
    // Later: `rest`, `metrics`, `keda`, `cluster`, `queues`.
}

/// The credential registry. NexQ is its own trust root, unrelated to AWS IAM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// Every principal allowed to talk to NexQ. Must not be empty.
    pub credentials: Vec<Credential>,
}

/// One principal's credentials.
///
/// Deliberately not named after AWS: the pair is an identifier plus a shared secret,
/// which is not an AWS-specific idea, and the same pair serves both facades in the two
/// ways they each expect to receive it:
///
/// - **SQS/SNS** — SigV4. The client sends [`Credential::key_id`] in the credential
///   scope and signs with [`Credential::secret`]; the server recomputes the HMAC and
///   compares. The secret must therefore be stored recoverably, not hashed, which is
///   why this is encrypted at rest rather than digested.
/// - **REST** — a bearer token, per its own simpler convention: the single opaque
///   string from [`Credential::bearer_token`], which is just the two halves joined.
///   That keeps one credential to issue and store, while still giving REST clients a
///   single value to paste into an `Authorization` header rather than a signing
///   procedure to implement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    /// Label for logs and metrics. Not used for authentication.
    pub name: String,

    /// Public identifier. Sent as the access key id by SigV4 clients.
    pub key_id: String,

    /// Shared secret. Never logged, never serialized.
    pub secret: Secret,
}

/// The SQS/SNS-compatible facade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsApiConfig {
    /// Whether to serve this facade at all. Each facade is independently switchable.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Address to bind. Defaults to `0.0.0.0:8080`.
    #[serde(default = "AwsApiConfig::default_bind_addr")]
    pub bind_addr: SocketAddr,

    /// Base URL clients should use to reach this facade, without a trailing slash.
    ///
    /// Queue URLs are built from this, and a client sends every subsequent request to
    /// the URL a queue operation handed back — so behind a proxy or an ingress this
    /// must be the externally reachable URL, not [`AwsApiConfig::bind_addr`].
    #[serde(default = "AwsApiConfig::default_public_base_url")]
    pub public_base_url: String,

    /// Account id embedded in queue URLs, defaulting to [`DEFAULT_ACCOUNT_ID`].
    ///
    /// NexQ has no accounts and attaches no meaning to this. It exists because real
    /// SQS queue URLs are `<host>/<account-id>/<queue-name>`, and clients, SDKs, and
    /// scripts that pick a URL apart expect to find something in that position.
    ///
    /// Must be exactly 12 digits, as AWS account ids are, so that a URL NexQ hands out
    /// is shaped like one a client has seen before. Validation rejects anything else at
    /// startup rather than letting it surface as an odd-looking URL later.
    ///
    /// **Changing this on a running deployment invalidates queue URLs clients already
    /// hold**: the account id is the one part of a URL that is checked when a request
    /// comes back, so a client reusing an old URL gets `InvalidAddress` until it looks
    /// the queue up again. Pick a value before anything depends on it.
    ///
    /// Accepted as a number as well as a string, since an all-digits value arrives
    /// from an environment variable or unquoted TOML already parsed as an integer.
    #[serde(
        default = "AwsApiConfig::default_account_id",
        deserialize_with = "deserialize_account_id"
    )]
    pub account_id: String,

    /// Region name used to build queue ARNs, defaulting to [`DEFAULT_REGION`].
    ///
    /// Used for **nothing else**. In particular it is not checked against the region a
    /// client signs with: SigV4 only needs signer and verifier to agree on a string, so
    /// any region works and always has. This exists because an ARN has a region-shaped
    /// slot in it — `arn:aws:sqs:<region>:<account-id>:<queue-name>` — and
    /// `GetQueueAttributes` has to put something there.
    ///
    /// Deriving it from each request's signature instead would mean two clients using
    /// different regions saw different ARNs for one queue, which is worse than a value
    /// an operator sets once.
    #[serde(default = "AwsApiConfig::default_region")]
    pub region: String,

    /// How far a request's signing timestamp may be from this server's clock before it
    /// is refused, in seconds. Defaults to [`DEFAULT_MAX_CLOCK_SKEW_SECS`].
    ///
    /// This is what stops a captured request from being replayed forever: the signature
    /// covers the timestamp, so an attacker cannot move it, and a window means an old
    /// request eventually stops being accepted.
    ///
    /// **`0` disables the check**, which leaves captured requests replayable
    /// indefinitely. That is a real trade an operator may have to make — an air-gapped
    /// deployment with no NTP can drift far enough to refuse honest clients — but it
    /// should be a deliberate choice, so the server says so at startup when it happens.
    #[serde(default = "AwsApiConfig::default_max_clock_skew_secs")]
    pub max_clock_skew_secs: u64,
}

/// A string that does not leak through `Debug`, `Display`, or serialization.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

fn default_true() -> bool {
    true
}

/// Number of digits in an account id, matching AWS.
const ACCOUNT_ID_DIGITS: usize = 12;

/// Default clock-skew window: 15 minutes, matching what AWS allows for SigV4, so a
/// client whose clock is good enough for real SQS is good enough here.
pub const DEFAULT_MAX_CLOCK_SKEW_SECS: u64 = 15 * 60;

/// Account id used when config does not name one.
///
/// All zeroes, which is what LocalStack uses, so tooling and runbooks written against
/// an SQS-compatible endpoint already expect to see it. Any 12-digit value works —
/// NexQ has no accounts and never interprets this — but see
/// [`AwsApiConfig::account_id`] before changing it on a running deployment.
pub const DEFAULT_ACCOUNT_ID: &str = "000000000000";

/// Region used in queue ARNs when config does not say. See [`AwsApiConfig::region`].
///
/// AWS's own default region in most tooling, so it is the least surprising thing to find
/// in an ARN from a server that has no opinion about regions.
pub const DEFAULT_REGION: &str = "us-east-1";

/// Deserialize an account id that may arrive already parsed as a number.
///
/// Environment values and unquoted TOML are parsed before they reach serde, so
/// `NEXQ_AWS_API__ACCOUNT_ID=000000000000` shows up as the integer `0`. Numbers are
/// therefore zero-padded back to the full width — otherwise the default account id
/// set through the environment would silently become `0`, and queue URLs with it.
fn deserialize_account_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AccountId {
        Text(String),
        Number(u64),
    }

    Ok(match AccountId::deserialize(deserializer)? {
        AccountId::Text(value) => value,
        AccountId::Number(value) => format!("{value:0width$}", width = ACCOUNT_ID_DIGITS),
    })
}

impl Config {
    /// Load config from the default file (or [`CONFIG_PATH_ENV`]) plus environment
    /// overrides.
    pub fn load() -> Result<Self, figment::Error> {
        Self::from_figment(Self::figment())
    }

    /// Load config from an explicit file path plus environment overrides.
    pub fn load_from(path: impl Into<PathBuf>) -> Result<Self, figment::Error> {
        Self::from_figment(Self::figment_from(path))
    }

    /// Extract and validate. Public so a caller that layered extra providers onto
    /// [`Config::figment`] still goes through the same validation.
    pub fn from_figment(figment: Figment) -> Result<Self, figment::Error> {
        let config: Self = figment.extract()?;
        config.validate()?;
        Ok(config)
    }

    /// The provider stack behind [`Config::load`], exposed so a caller can layer
    /// something extra on top — CLI flags, say — before extracting.
    pub fn figment() -> Figment {
        let path = std::env::var(CONFIG_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_FILE));
        Self::figment_from(path)
    }

    /// [`Config::figment`] with the config file path given explicitly.
    pub fn figment_from(path: impl Into<PathBuf>) -> Figment {
        Figment::new()
            .merge(Toml::file(path.into()))
            .merge(Self::env_provider())
    }

    /// Environment overrides.
    ///
    /// An override must name a nested key — `NEXQ_AWS_API__BIND_ADDR`, not
    /// `NEXQ_BIND_ADDR` — and anything without the separator is ignored. Without that
    /// filter, every unrelated `NEXQ_`-prefixed variable in the environment becomes an
    /// unknown top-level key and fails the load; the devcontainer alone exports three
    /// (`NEXQ_POSTGRES_URL` and friends), and `NEXQ_CONFIG` is read separately here.
    ///
    /// Every real key is nested under a facade or `auth`, so nothing is unreachable.
    fn env_provider() -> Env {
        Env::prefixed(ENV_PREFIX)
            .filter(|key| key.as_str().contains(ENV_NESTED_SEPARATOR))
            .split(ENV_NESTED_SEPARATOR)
    }

    /// Checks that can't be expressed as types.
    fn validate(&self) -> Result<(), figment::Error> {
        let account_id = &self.aws_api.account_id;
        if account_id.len() != ACCOUNT_ID_DIGITS
            || !account_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!(
                "aws_api.account_id must be exactly {ACCOUNT_ID_DIGITS} digits, as a real \
                 SQS account id is; got {account_id:?}"
            )
            .into());
        }

        // An empty region would produce `arn:aws:sqs::000000000000:jobs`, which parses
        // as an ARN with no region and would quietly mislead anything reading it.
        if self.aws_api.region.trim().is_empty() {
            return Err(
                "aws_api.region must not be empty: it names the region slot in \
                        queue ARNs"
                    .into(),
            );
        }

        if self.auth.credentials.is_empty() {
            return Err("auth.credentials must contain at least one entry"
                .to_owned()
                .into());
        }

        for (index, credential) in self.auth.credentials.iter().enumerate() {
            if credential.key_id.is_empty() {
                return Err(format!("auth.credentials[{index}].key_id must not be empty").into());
            }
            if self
                .auth
                .credentials
                .iter()
                .filter(|other| other.key_id == credential.key_id)
                .count()
                > 1
            {
                return Err(format!(
                    "auth.credentials contains a duplicate key_id: {}",
                    credential.key_id
                )
                .into());
            }
        }

        Ok(())
    }
}

impl AuthConfig {
    /// Look up a principal by the identifier a client presented.
    pub fn credential(&self, key_id: &str) -> Option<&Credential> {
        self.auth_credentials()
            .find(|credential| credential.key_id == key_id)
    }

    fn auth_credentials(&self) -> impl Iterator<Item = &Credential> {
        self.credentials.iter()
    }
}

impl Credential {
    /// The single opaque string a REST client sends as `Authorization: Bearer <token>`.
    ///
    /// Derived rather than stored, so there is nothing extra to issue, rotate, or leak.
    /// The identifier comes first so the server can split it off, find the principal,
    /// and then compare the secret.
    pub fn bearer_token(&self) -> String {
        format!("{}{}{}", self.key_id, BEARER_TOKEN_SEPARATOR, self.secret.0)
    }
}

/// Separates identifier from secret in a REST bearer token. Not valid in a `key_id`
/// that a SigV4 client could send, so the split is unambiguous.
pub const BEARER_TOKEN_SEPARATOR: char = '.';

impl AwsApiConfig {
    fn default_bind_addr() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080))
    }

    fn default_public_base_url() -> String {
        "http://localhost:8080".to_owned()
    }

    fn default_account_id() -> String {
        DEFAULT_ACCOUNT_ID.to_owned()
    }

    fn default_region() -> String {
        DEFAULT_REGION.to_owned()
    }

    fn default_max_clock_skew_secs() -> u64 {
        DEFAULT_MAX_CLOCK_SKEW_SECS
    }

    /// The clock-skew window, or `None` if the check is switched off.
    pub fn max_clock_skew(&self) -> Option<Duration> {
        match self.max_clock_skew_secs {
            0 => None,
            seconds => Some(Duration::from_secs(seconds)),
        }
    }

    /// The configured base URL with any trailing slashes removed, so callers can
    /// join paths onto it without doubling the separator.
    pub fn base_url(&self) -> &str {
        self.public_base_url.trim_end_matches('/')
    }
}

impl Default for AwsApiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_addr: Self::default_bind_addr(),
            public_base_url: Self::default_public_base_url(),
            account_id: Self::default_account_id(),
            region: Self::default_region(),
            max_clock_skew_secs: Self::default_max_clock_skew_secs(),
        }
    }
}

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret itself. Named to make its use easy to spot in review.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Serializes as a placeholder, so a round-tripped or dumped config never carries
/// the real value. [`Secret::expose`] is the only way out.
impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("<redacted>")
    }
}

/// Lets a [`Config`] be layered back into a [`Figment`], e.g. as the base for a test
/// or for a caller assembling its own provider stack.
impl Provider for Config {
    fn metadata(&self) -> Metadata {
        Metadata::named("NexQ config")
    }

    fn data(&self) -> Result<figment::value::Map<Profile, figment::value::Dict>, figment::Error> {
        figment::providers::Serialized::defaults(self).data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::Jail;

    const MINIMAL: &str = r#"
        [[auth.credentials]]
        name = "dev"
        key_id = "AKIANEXQEXAMPLE"
        secret = "shhh"
    "#;

    #[test]
    fn defaults_apply_when_only_credentials_are_given() {
        Jail::expect_with(|jail| {
            jail.create_file(DEFAULT_CONFIG_FILE, MINIMAL)?;
            let config = Config::load().expect("load");

            assert!(config.aws_api.enabled);
            assert_eq!(config.aws_api.bind_addr.to_string(), "0.0.0.0:8080");
            assert_eq!(config.aws_api.base_url(), "http://localhost:8080");
            assert_eq!(config.aws_api.account_id, DEFAULT_ACCOUNT_ID);
            assert_eq!(
                DEFAULT_ACCOUNT_ID.len(),
                ACCOUNT_ID_DIGITS,
                "the default must satisfy the rule it is validated against"
            );

            let credential = config.auth.credential("AKIANEXQEXAMPLE").expect("lookup");
            assert_eq!(credential.name, "dev");
            assert_eq!(credential.secret.expose(), "shhh");
            assert!(config.auth.credential("AKIAWRONG").is_none());
            Ok(())
        });
    }

    #[test]
    fn file_values_override_defaults() {
        Jail::expect_with(|jail| {
            jail.create_file(
                DEFAULT_CONFIG_FILE,
                &format!(
                    r#"
                    {MINIMAL}

                    [aws_api]
                    enabled = false
                    bind_addr = "127.0.0.1:9999"
                    public_base_url = "https://queue.example.com/"
                    account_id = "123456789012"
                    "#
                ),
            )?;
            let config = Config::load().expect("load");

            assert!(!config.aws_api.enabled);
            assert_eq!(config.aws_api.bind_addr.to_string(), "127.0.0.1:9999");
            // Trailing slash trimmed so paths can be joined without doubling it.
            assert_eq!(config.aws_api.base_url(), "https://queue.example.com");
            assert_eq!(config.aws_api.account_id, "123456789012");
            Ok(())
        });
    }

    #[test]
    fn env_overrides_file() {
        Jail::expect_with(|jail| {
            jail.create_file(DEFAULT_CONFIG_FILE, MINIMAL)?;
            jail.set_env("NEXQ_AWS_API__BIND_ADDR", "127.0.0.1:1234");
            jail.set_env("NEXQ_AWS_API__ACCOUNT_ID", "999999999999");
            let config = Config::load().expect("load");

            assert_eq!(config.aws_api.bind_addr.to_string(), "127.0.0.1:1234");
            assert_eq!(config.aws_api.account_id, "999999999999");
            Ok(())
        });
    }

    #[test]
    fn an_all_zero_account_id_survives_the_environment() {
        Jail::expect_with(|jail| {
            jail.create_file(DEFAULT_CONFIG_FILE, MINIMAL)?;
            // Arrives as the integer 0, having been parsed before serde sees it.
            jail.set_env("NEXQ_AWS_API__ACCOUNT_ID", "000000000000");
            let config = Config::load().expect("load");

            assert_eq!(config.aws_api.account_id, "000000000000");
            Ok(())
        });
    }

    #[test]
    fn a_malformed_account_id_is_an_error() {
        Jail::expect_with(|jail| {
            jail.create_file(
                DEFAULT_CONFIG_FILE,
                &format!("{MINIMAL}\n[aws_api]\naccount_id = \"not-an-account\"\n"),
            )?;
            let error = Config::load().expect_err("account ids are 12 digits");

            assert!(error.to_string().contains("account_id"), "{error}");
            Ok(())
        });
    }

    #[test]
    fn credentials_can_come_from_the_environment() {
        Jail::expect_with(|jail| {
            jail.set_env(
                "NEXQ_AUTH__CREDENTIALS",
                r#"[{name="env",key_id="AKIAENVONLY",secret="shhh"}]"#,
            );
            let config = Config::load().expect("load");

            let credential = config.auth.credential("AKIAENVONLY").expect("lookup");
            assert_eq!(credential.name, "env");
            Ok(())
        });
    }

    #[test]
    fn config_path_env_selects_the_file() {
        Jail::expect_with(|jail| {
            jail.create_file("elsewhere.toml", MINIMAL)?;
            jail.set_env(CONFIG_PATH_ENV, "elsewhere.toml");
            let config = Config::load().expect("load");

            assert!(config.auth.credential("AKIANEXQEXAMPLE").is_some());
            Ok(())
        });
    }

    #[test]
    fn unrelated_nexq_env_vars_are_ignored() {
        Jail::expect_with(|jail| {
            jail.create_file(DEFAULT_CONFIG_FILE, MINIMAL)?;
            // The devcontainer exports these for its dev services; they are not config.
            jail.set_env("NEXQ_POSTGRES_URL", "postgres://localhost/nexq");
            jail.set_env("NEXQ_OPENSEARCH_URL", "http://localhost:9200");
            jail.set_env("NEXQ_ELASTICSEARCH_URL", "http://localhost:9200");
            Config::load().expect("stray NEXQ_ vars must not break the load");
            Ok(())
        });
    }

    #[test]
    fn missing_credentials_is_an_error() {
        Jail::expect_with(|jail| {
            jail.create_file(
                DEFAULT_CONFIG_FILE,
                "[aws_api]\nbind_addr = \"0.0.0.0:80\"\n",
            )?;
            let error = Config::load().expect_err("credentials are required");

            assert!(
                error.to_string().contains("auth"),
                "error should name the missing key: {error}"
            );
            Ok(())
        });
    }

    #[test]
    fn an_empty_credential_list_is_an_error() {
        Jail::expect_with(|jail| {
            jail.create_file(DEFAULT_CONFIG_FILE, "[auth]\ncredentials = []\n")?;
            let error = Config::load().expect_err("an empty registry authenticates nobody");

            assert!(error.to_string().contains("at least one"), "{error}");
            Ok(())
        });
    }

    #[test]
    fn duplicate_key_ids_are_an_error() {
        Jail::expect_with(|jail| {
            jail.create_file(
                DEFAULT_CONFIG_FILE,
                &format!("{MINIMAL}\n{}", MINIMAL.replace("\"dev\"", "\"other\"")),
            )?;
            let error = Config::load().expect_err("a key_id must identify one principal");

            assert!(error.to_string().contains("duplicate key_id"), "{error}");
            Ok(())
        });
    }

    #[test]
    fn unknown_keys_are_rejected() {
        Jail::expect_with(|jail| {
            jail.create_file(
                DEFAULT_CONFIG_FILE,
                &format!("{MINIMAL}\n[aws_api]\nbnid_addr = \"0.0.0.0:80\"\n"),
            )?;
            Config::load().expect_err("typo should not be silently ignored");
            Ok(())
        });
    }

    #[test]
    fn bearer_token_carries_the_key_id_then_the_secret() {
        let credential = Credential {
            name: "dev".to_owned(),
            key_id: "AKIANEXQEXAMPLE".to_owned(),
            secret: Secret::new("shhh"),
        };

        let token = credential.bearer_token();
        let (key_id, secret) = token
            .split_once(BEARER_TOKEN_SEPARATOR)
            .expect("token is splittable");

        assert_eq!(key_id, "AKIANEXQEXAMPLE");
        assert_eq!(secret, "shhh");
    }

    /// Keeps the shipped example from drifting out of sync with these types.
    ///
    /// Runs inside a jail even though it reads a fixed path: [`Jail`] sets environment
    /// variables process-wide and holds a global lock while it does, so a test that
    /// loads config outside one races the `NEXQ_*` overrides the jailed tests set.
    /// `clear_env` then keeps the example — not the ambient environment — the only input.
    #[test]
    fn the_example_config_is_valid() {
        Jail::expect_with(|jail| {
            jail.clear_env();
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../nexq.example.toml");
            let config = Config::load_from(path).expect("nexq.example.toml must stay loadable");

            assert_eq!(config.auth.credentials.len(), 1);
            assert!(config.aws_api.enabled);
            assert_eq!(config.aws_api.base_url(), "http://localhost:8080");
            assert_eq!(config.aws_api.account_id, "000000000000");
            Ok(())
        });
    }

    #[test]
    fn secrets_do_not_leak_through_debug_or_serialization() {
        let secret = Secret::new("shhh");

        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        let dumped = figment::providers::Serialized::defaults(&secret);
        assert!(!format!("{:?}", dumped.data()).contains("shhh"));
    }
}
