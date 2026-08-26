//! Turning TLS config into something rustls can serve with.
//!
//! Here rather than in a facade because every facade needs the same thing, and because a
//! second copy of certificate loading is a second place for the error messages to be
//! unhelpful. The facades decide *whether* to serve TLS; this decides what a certificate
//! file means.
//!
//! Errors are the point of this module as much as the loading is. A misconfigured
//! certificate is one of the least self-explanatory failures an operator meets — the
//! symptom is usually a client saying nothing more than "handshake failed" — so every
//! error here says which file, and what was wrong with it.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use thiserror::Error;

use crate::config::{ClientTlsConfig, ServerTlsConfig};

/// Why TLS could not be set up.
#[derive(Debug, Error)]
pub enum TlsError {
    #[error("cannot read {path}: {source}")]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} contains no {wanted}")]
    Empty { path: PathBuf, wanted: &'static str },

    #[error("{path} holds {count} private keys; it should hold exactly one")]
    ManyKeys { path: PathBuf, count: usize },

    /// rustls rejected the material — a key that does not match the certificate, an
    /// unsupported algorithm, a malformed certificate.
    #[error("rustls rejected the TLS configuration: {0}")]
    Rejected(#[source] rustls::Error),

    #[error("the client certificate authority in {path} is not usable: {source}")]
    BadClientCa {
        path: PathBuf,
        #[source]
        source: rustls::Error,
    },
}

/// Build a rustls server configuration from certificate files.
///
/// Done at startup rather than on the first connection, so a bad certificate stops the
/// server coming up instead of being discovered by whoever connects first.
pub fn server_config(config: &ServerTlsConfig) -> Result<Arc<ServerConfig>, TlsError> {
    let certificates = certificates(&config.certificate)?;
    let key = private_key(&config.private_key)?;

    // The provider is named rather than left to rustls to infer. Inferring works only
    // while exactly one provider is compiled in, and a future dependency enabling a
    // second one would turn that into a panic on the first connection.
    let builder =
        ServerConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
            .with_safe_default_protocol_versions()
            .map_err(TlsError::Rejected)?;

    let builder = match &config.client_ca {
        // Mutual TLS: a connection without a certificate this authority signed is
        // refused during the handshake, before any request is read.
        Some(path) => {
            let mut roots = RootCertStore::empty();
            add_roots(&mut roots, path)?;

            let verifier = WebPkiClientVerifier::builder(roots.into())
                .build()
                .map_err(|error| TlsError::BadClientCa {
                    path: path.clone(),
                    source: rustls::Error::General(error.to_string()),
                })?;

            builder.with_client_cert_verifier(verifier)
        }
        None => builder.with_no_client_auth(),
    };

    let mut server = builder
        .with_single_cert(certificates, key)
        .map_err(TlsError::Rejected)?;

    // HTTP/2 first, then HTTP/1.1 — what a client negotiates over TLS, and the order
    // says which NexQ prefers. Without this every client falls back to HTTP/1.1.
    server.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(Arc::new(server))
}

/// Build a rustls client configuration from a trust bundle.
///
/// For NexQ acting as a client. Nothing calls it yet — see [`ClientTlsConfig`] — but the
/// loading is exercised by this module's tests, so the plumbing is known to work before
/// something depends on it.
pub fn client_config(config: &ClientTlsConfig) -> Result<Arc<ClientConfig>, TlsError> {
    let mut roots = RootCertStore::empty();
    add_roots(&mut roots, &config.ca_bundle)?;

    let client =
        ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
            .with_safe_default_protocol_versions()
            .map_err(TlsError::Rejected)?
            .with_root_certificates(roots)
            .with_no_client_auth();

    Ok(Arc::new(client))
}

/// Read a PEM certificate chain.
fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let mut reader = open(path)?;

    let certificates: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<_, _>>()
        .map_err(|source| TlsError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?;

    if certificates.is_empty() {
        return Err(TlsError::Empty {
            path: path.to_path_buf(),
            wanted: "PEM certificates",
        });
    }

    Ok(certificates)
}

/// Read exactly one PEM private key.
///
/// More than one is refused rather than the first being taken: a file holding two keys
/// means the operator believes something that is not true about which one is in use, and
/// guessing would make that belief harder to correct.
fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let mut reader = open(path)?;

    let mut keys: Vec<PrivateKeyDer<'static>> = Vec::new();
    for item in rustls_pemfile::read_all(&mut reader) {
        let item = item.map_err(|source| TlsError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?;

        // All three encodings a private key comes in, since which one a tool emits is
        // not something an operator chose.
        match item {
            rustls_pemfile::Item::Pkcs1Key(key) => keys.push(key.into()),
            rustls_pemfile::Item::Pkcs8Key(key) => keys.push(key.into()),
            rustls_pemfile::Item::Sec1Key(key) => keys.push(key.into()),
            _ => {}
        }
    }

    match keys.len() {
        0 => Err(TlsError::Empty {
            path: path.to_path_buf(),
            wanted: "PEM private key",
        }),
        1 => Ok(keys.remove(0)),
        count => Err(TlsError::ManyKeys {
            path: path.to_path_buf(),
            count,
        }),
    }
}

/// Add every certificate in a PEM bundle to a trust store.
fn add_roots(roots: &mut RootCertStore, path: &Path) -> Result<(), TlsError> {
    for certificate in certificates(path)? {
        roots
            .add(certificate)
            .map_err(|source| TlsError::BadClientCa {
                path: path.to_path_buf(),
                source,
            })?;
    }

    Ok(())
}

fn open(path: &Path) -> Result<BufReader<File>, TlsError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| TlsError::Unreadable {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// Generate a self-signed certificate and key with `openssl`, returning their paths.
    ///
    /// Real files through a real tool, rather than fixtures checked into the repository:
    /// a committed certificate expires, and a test that starts failing on a date is worse
    /// than one that needs `openssl` on the path.
    fn self_signed(directory: &Path, name: &str) -> (PathBuf, PathBuf) {
        let certificate = directory.join(format!("{name}.pem"));
        let key = directory.join(format!("{name}.key"));

        let status = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            ])
            .arg("-subj")
            .arg(format!("/CN={name}"))
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&certificate)
            .output()
            .expect("openssl should be installed");
        assert!(
            status.status.success(),
            "openssl failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        (certificate, key)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("nexq-tls-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create temp dir");

        directory
    }

    #[test]
    fn a_certificate_and_key_make_a_server_config() {
        let directory = temp_dir("server");
        let (certificate, private_key) = self_signed(&directory, "nexq.test");

        let config = server_config(&ServerTlsConfig {
            certificate,
            private_key,
            client_ca: None,
        })
        .expect("a self-signed pair should load");

        assert_eq!(
            config.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            "HTTP/2 should be offered first, or every client falls back to 1.1"
        );
    }

    #[test]
    fn a_client_ca_turns_on_mutual_tls() {
        let directory = temp_dir("mtls");
        let (certificate, private_key) = self_signed(&directory, "nexq.test");
        let (client_ca, _) = self_signed(&directory, "clients");

        // The difference is inside rustls's verifier, so what is asserted here is that
        // the material is accepted at all. That mutual TLS actually *refuses* an
        // uncertificated client is asserted end to end by the acceptance suite.
        server_config(&ServerTlsConfig {
            certificate,
            private_key,
            client_ca: Some(client_ca),
        })
        .expect("a client CA should be accepted");
    }

    #[test]
    fn a_ca_bundle_makes_a_client_config() {
        let directory = temp_dir("client");
        let (ca_bundle, _) = self_signed(&directory, "authority");

        client_config(&ClientTlsConfig { ca_bundle }).expect("a CA bundle should load");
    }

    #[test]
    fn a_missing_file_says_which_setting_and_which_path() {
        let directory = temp_dir("missing");
        let (certificate, private_key) = self_signed(&directory, "nexq.test");

        let error = server_config(&ServerTlsConfig {
            certificate: directory.join("nope.pem"),
            private_key: private_key.clone(),
            client_ca: None,
        })
        .expect_err("no such certificate");
        assert!(error.to_string().contains("nope.pem"), "{error}");

        let error = server_config(&ServerTlsConfig {
            certificate,
            private_key: directory.join("nope.key"),
            client_ca: None,
        })
        .expect_err("no such key");
        assert!(error.to_string().contains("nope.key"), "{error}");
    }

    #[test]
    fn a_file_with_no_certificate_in_it_is_refused() {
        let directory = temp_dir("empty");
        let (_, private_key) = self_signed(&directory, "nexq.test");
        let empty = directory.join("empty.pem");
        std::fs::write(&empty, "not a certificate\n").expect("write");

        let error = server_config(&ServerTlsConfig {
            certificate: empty,
            private_key,
            client_ca: None,
        })
        .expect_err("no certificates in there");

        assert!(
            error.to_string().contains("no PEM certificates"),
            "the error should say what was missing: {error}"
        );
    }

    #[test]
    fn a_file_with_no_key_in_it_is_refused() {
        let directory = temp_dir("nokey");
        let (certificate, _) = self_signed(&directory, "nexq.test");

        // The certificate is a valid PEM file that simply is not a key, which is a
        // plausible mix-up and should say so rather than failing obscurely.
        let error = server_config(&ServerTlsConfig {
            certificate: certificate.clone(),
            private_key: certificate,
            client_ca: None,
        })
        .expect_err("that is a certificate, not a key");

        assert!(error.to_string().contains("no PEM private key"), "{error}");
    }

    #[test]
    fn two_keys_in_one_file_are_refused_rather_than_guessed_between() {
        let directory = temp_dir("twokeys");
        let (certificate, first) = self_signed(&directory, "one");
        let (_, second) = self_signed(&directory, "two");

        let both = directory.join("both.key");
        let contents = format!(
            "{}{}",
            std::fs::read_to_string(&first).expect("read"),
            std::fs::read_to_string(&second).expect("read")
        );
        std::fs::write(&both, contents).expect("write");

        let error = server_config(&ServerTlsConfig {
            certificate,
            private_key: both,
            client_ca: None,
        })
        .expect_err("two keys");

        assert!(error.to_string().contains("2 private keys"), "{error}");
    }

    #[test]
    fn a_key_that_does_not_match_the_certificate_is_refused() {
        // The mistake that produces the least helpful symptom in the wild: everything
        // loads, and then every handshake fails. Caught at startup instead.
        let directory = temp_dir("mismatch");
        let (certificate, _) = self_signed(&directory, "one");
        let (_, other_key) = self_signed(&directory, "two");

        server_config(&ServerTlsConfig {
            certificate,
            private_key: other_key,
            client_ca: None,
        })
        .expect_err("the key belongs to a different certificate");
    }
}
