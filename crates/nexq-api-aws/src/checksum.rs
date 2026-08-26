//! Message body checksums.
//!
//! SQS reports an MD5 of the body on both send and receive, and SDKs *verify* it — a
//! missing or wrong value makes a client raise rather than accept the message. So this
//! is not decoration: it is part of being compatible.
//!
//! MD5 is not being used as a security primitive here. It detects a body mangled in
//! transit, and the algorithm is fixed by the wire format rather than chosen.

use md5::{Digest, Md5};

/// Hex-encoded MD5 of a message body, as SQS reports it.
pub fn md5_of_body(body: &str) -> String {
    hex::encode(Md5::digest(body.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_md5_values() {
        // Published vectors, so this is checked against MD5 itself rather than against
        // whatever this code happens to produce.
        assert_eq!(md5_of_body(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_of_body("hello"), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(
            md5_of_body("The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn is_hex_of_the_expected_width() {
        let digest = md5_of_body("anything");

        assert_eq!(digest.len(), 32);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(
            digest.chars().all(|c| !c.is_ascii_uppercase()),
            "SQS reports lowercase hex"
        );
    }

    #[test]
    fn covers_multibyte_bodies_by_their_bytes() {
        // Hashing chars rather than bytes would give a different answer for the same
        // wire content.
        assert_eq!(md5_of_body("é"), "66ddcd97cfdeabb2f6fb8a999b4bc76f");
    }
}
